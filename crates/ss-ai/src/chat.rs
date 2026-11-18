//! The assistant chat: a conversation with the configured AI, shown in its
//! own native window. Each app supplies its look ([`ChatTheme`]) and the
//! forecast context the assistant answers from.

use crossbeam_channel::{Receiver, TryRecvError};
use egui::{
    Align, Color32, Context, CornerRadius, FontId, Key, Layout, Margin, RichText, ScrollArea,
    Stroke, Ui, ViewportBuilder, ViewportClass, ViewportId,
};
use ss_core::ai::{AiEndpoint, AiSettings, AI_DISCLAIMER};
use tokio::runtime::Handle;

use crate::{complete, Message, Role};

/// Colours and type for the chat window, from the host app's palette.
#[derive(Debug, Clone)]
pub struct ChatTheme {
    pub bg: Color32,
    pub surface: Color32,
    pub border: Color32,
    pub text: Color32,
    pub muted: Color32,
    pub accent: Color32,
    /// Fill of the user's own messages.
    pub user_bubble: Color32,
    pub danger: Color32,
    pub corner: u8,
    /// Small caption font (e.g. a spaced mono).
    pub caption: FontId,
}

/// Something the chat asks its host to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatAction {
    None,
    OpenSettings,
}

/// Starter questions shown in an empty chat.
pub const SUGGESTIONS: [&str; 4] = [
    "What's the weather like right now?",
    "Will it rain in the next few hours?",
    "Is it a good day for a run this afternoon?",
    "What should I wear tomorrow?",
];

#[derive(Default)]
pub struct Chat {
    pub open: bool,
    messages: Vec<Message>,
    input: String,
    pending: Option<Receiver<Result<String, String>>>,
    error: Option<String>,
    scroll_to_end: bool,
    focus_input: bool,
    /// This conversation's id (OpenCode's `x-opencode-session`); renewed
    /// when the chat is cleared.
    session: Option<String>,
    /// The app showing this chat (for the request's user agent).
    app: String,
}

impl Chat {
    pub fn show_window(&mut self) {
        self.open = true;
        self.focus_input = true;
    }

    pub fn is_busy(&self) -> bool {
        self.pending.is_some()
    }

    pub fn messages(&self) -> &[Message] {
        &self.messages
    }

    pub fn clear(&mut self) {
        self.session = None;
        self.messages.clear();
        self.error = None;
        self.pending = None;
    }

    /// Add the user's message and ask the AI. `system` carries the context.
    pub fn send(
        &mut self,
        text: &str,
        endpoint: AiEndpoint,
        system: String,
        rt: &Handle,
        client: &reqwest::Client,
        ctx: &Context,
    ) {
        let text = text.trim();
        if text.is_empty() || self.is_busy() {
            return;
        }
        self.messages.push(Message::user(text));
        self.error = None;
        self.scroll_to_end = true;
        let (tx, rx) = crossbeam_channel::bounded(1);
        self.pending = Some(rx);
        let history = self.messages.clone();
        let session = self
            .session
            .get_or_insert_with(crate::new_session_id)
            .clone();
        let endpoint = endpoint.for_app(&self.app).in_session(&session);
        let (client, ctx) = (client.clone(), ctx.clone());
        rt.spawn(async move {
            let r = complete(&client, &endpoint, &system, &history).await;
            let _ = tx.send(r);
            ctx.request_repaint();
        });
    }

    /// Collect a finished reply, if any.
    pub fn poll(&mut self) {
        let Some(rx) = &self.pending else { return };
        match rx.try_recv() {
            Ok(Ok(text)) => {
                self.messages.push(Message::assistant(text));
                self.pending = None;
                self.scroll_to_end = true;
            }
            Ok(Err(e)) => {
                self.error = Some(e);
                self.pending = None;
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                self.error = Some("The request was interrupted.".into());
                self.pending = None;
            }
        }
    }

    /// Draw the chat in its own window (when open). `system` is called only
    /// when a message is sent, to build the prompt with fresh context.
    #[allow(clippy::too_many_arguments)]
    pub fn show(
        &mut self,
        ctx: &Context,
        app_name: &str,
        theme: &ChatTheme,
        settings: &AiSettings,
        context_label: &str,
        system: impl FnOnce() -> String,
        rt: &Handle,
        client: &reqwest::Client,
    ) -> ChatAction {
        self.poll();
        self.app = app_name.to_string();
        if !self.open {
            return ChatAction::None;
        }
        let mut action = ChatAction::None;
        let mut system = Some(system);
        let builder = ViewportBuilder::default()
            .with_title(format!("Assistant — {app_name}"))
            .with_inner_size([460.0, 680.0])
            .with_min_inner_size([340.0, 420.0]);
        ctx.show_viewport_immediate(
            ViewportId::from_hash_of("ss-assistant"),
            builder,
            |ui, class| {
                let vctx = ui.ctx().clone();
                if class != ViewportClass::EmbeddedWindow
                    && vctx.input(|i| i.viewport().close_requested())
                {
                    self.open = false;
                }
                let frame = egui::Frame::NONE
                    .fill(theme.bg)
                    .inner_margin(Margin::same(14));
                egui::CentralPanel::default().frame(frame).show(ui, |ui| {
                    if let Some(a) =
                        self.body(ui, theme, settings, context_label, &mut system, rt, client)
                    {
                        action = a;
                    }
                });
            },
        );
        if self.pending.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(120));
        }
        action
    }

    #[allow(clippy::too_many_arguments)]
    fn body(
        &mut self,
        ui: &mut Ui,
        t: &ChatTheme,
        settings: &AiSettings,
        context_label: &str,
        system: &mut Option<impl FnOnce() -> String>,
        rt: &Handle,
        client: &reqwest::Client,
    ) -> Option<ChatAction> {
        let mut action = None;
        let corner = CornerRadius::same(t.corner);
        // Header.
        ui.horizontal(|ui| {
            ui.label(RichText::new("Weather assistant").size(18.0).color(t.text));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui
                    .add_enabled(!self.messages.is_empty(), egui::Button::new("New chat"))
                    .clicked()
                {
                    self.clear();
                }
                if ui.button("AI settings").clicked() {
                    action = Some(ChatAction::OpenSettings);
                }
            });
        });
        let model = match settings.endpoint() {
            Some(e) => format!("{} · {}", settings.provider.label(), e.model),
            None => settings.provider.label().to_string(),
        };
        ui.label(
            RichText::new(format!("{model} · answers about {context_label}"))
                .font(t.caption.clone())
                .color(t.muted),
        );
        // The standing disclaimer.
        egui::Frame::new()
            .fill(Color32::from_rgba_unmultiplied(
                t.accent.r(),
                t.accent.g(),
                t.accent.b(),
                24,
            ))
            .stroke(Stroke::new(1.0, t.border))
            .corner_radius(corner)
            .inner_margin(Margin::symmetric(10, 8))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.add(
                    egui::Label::new(
                        RichText::new(format!("⚠ {AI_DISCLAIMER}"))
                            .size(12.5)
                            .color(t.muted),
                    )
                    .wrap(),
                );
            });
        ui.add_space(6.0);

        let endpoint = settings.endpoint();
        let input_h = 88.0;
        let list_h = (ui.available_height() - input_h).max(80.0);

        // Conversation.
        ScrollArea::vertical()
            .id_salt("chat_log")
            .max_height(list_h)
            .min_scrolled_height(list_h)
            .auto_shrink([false, false])
            .stick_to_bottom(true)
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 10.0;
                if let Some(problem) = settings.problem() {
                    ui.add_space(20.0);
                    ui.vertical_centered(|ui| {
                        ui.label(RichText::new(problem).color(t.muted));
                        if ui.button("Open AI settings").clicked() {
                            action = Some(ChatAction::OpenSettings);
                        }
                    });
                } else if self.messages.is_empty() {
                    ui.add_space(10.0);
                    ui.label(RichText::new("Ask about the forecast, for example:").color(t.muted));
                    for s in SUGGESTIONS {
                        if ui
                            .add(egui::Button::new(RichText::new(s).color(t.text)).wrap())
                            .clicked()
                        {
                            self.input = s.to_string();
                            self.focus_input = true;
                        }
                    }
                }
                for m in &self.messages {
                    bubble(ui, t, m);
                }
                if self.is_busy() {
                    ui.horizontal(|ui| {
                        ui.add(egui::Spinner::new().size(14.0).color(t.accent));
                        ui.label(RichText::new("Thinking…").color(t.muted));
                    });
                }
                if let Some(e) = &self.error {
                    ui.add(egui::Label::new(RichText::new(e).color(t.danger)).wrap());
                }
                if std::mem::take(&mut self.scroll_to_end) {
                    ui.scroll_to_cursor(Some(Align::BOTTOM));
                }
            });

        // Input.
        ui.add_space(6.0);
        let mut send_now = false;
        ui.horizontal(|ui| {
            let w = ui.available_width() - 76.0;
            let edit = egui::TextEdit::multiline(&mut self.input)
                .hint_text("Ask about the weather… (Enter to send, Shift+Enter for a new line)")
                .desired_rows(2)
                .desired_width(w)
                .lock_focus(true);
            let r = ui.add_enabled(endpoint.is_some(), edit);
            if std::mem::take(&mut self.focus_input) {
                r.request_focus();
            }
            if r.has_focus() && ui.input(|i| i.key_pressed(Key::Enter) && !i.modifiers.shift) {
                // Drop the newline the key just inserted.
                while self.input.ends_with('\n') {
                    self.input.pop();
                }
                send_now = true;
            }
            let can_send = endpoint.is_some() && !self.is_busy() && !self.input.trim().is_empty();
            if ui
                .add_enabled(
                    can_send,
                    egui::Button::new("Send").min_size(egui::vec2(68.0, 40.0)),
                )
                .clicked()
            {
                send_now = true;
            }
        });
        if send_now && !self.is_busy() {
            if let (Some(e), Some(sys)) = (endpoint, system.take()) {
                let text = std::mem::take(&mut self.input);
                let ctx = ui.ctx().clone();
                self.send(&text, e, sys(), rt, client, &ctx);
                self.focus_input = true;
            }
        }
        action
    }
}

fn bubble(ui: &mut Ui, t: &ChatTheme, m: &Message) {
    let user = m.role == Role::User;
    let max_w = (ui.available_width() * 0.86).max(120.0);
    let layout = if user {
        Layout::right_to_left(Align::Min)
    } else {
        Layout::left_to_right(Align::Min)
    };
    ui.with_layout(layout, |ui| {
        ui.set_max_width(ui.available_width());
        egui::Frame::new()
            .fill(if user { t.user_bubble } else { t.surface })
            .stroke(Stroke::new(1.0, t.border))
            .corner_radius(CornerRadius::same(t.corner))
            .inner_margin(Margin::symmetric(12, 9))
            .show(ui, |ui| {
                ui.set_max_width(max_w);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 3.0;
                    ui.label(
                        RichText::new(if user { "You" } else { "✦ Assistant" })
                            .font(t.caption.clone())
                            .color(if user { t.muted } else { t.accent }),
                    );
                    ui.add(egui::Label::new(RichText::new(&m.text).color(t.text)).wrap());
                    if ui
                        .small_button("Copy")
                        .on_hover_text("Copy this message")
                        .clicked()
                    {
                        ui.ctx().copy_text(m.text.clone());
                    }
                });
            });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn poll_appends_replies_and_errors() {
        let mut c = Chat::default();
        c.messages.push(Message::user("hi"));
        let (tx, rx) = crossbeam_channel::bounded(1);
        c.pending = Some(rx);
        assert!(c.is_busy());
        c.poll();
        assert!(c.is_busy(), "nothing yet");
        tx.send(Ok("hello".into())).unwrap();
        c.poll();
        assert!(!c.is_busy());
        assert_eq!(c.messages().last().unwrap(), &Message::assistant("hello"));

        let (tx, rx) = crossbeam_channel::bounded(1);
        c.pending = Some(rx);
        tx.send(Err("nope".into())).unwrap();
        c.poll();
        assert_eq!(c.error.as_deref(), Some("nope"));
        c.clear();
        assert!(c.messages().is_empty() && c.error.is_none());
    }

    #[test]
    fn dropped_request_reports_an_error() {
        let mut c = Chat::default();
        let (tx, rx) = crossbeam_channel::bounded::<Result<String, String>>(1);
        c.pending = Some(rx);
        drop(tx);
        c.poll();
        assert!(!c.is_busy());
        assert!(c.error.is_some());
    }
}
