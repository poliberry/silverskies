//! The assistant chat: a conversation with the configured AI, shown in its
//! own native window. Each app supplies its look ([`ChatTheme`]) and the
//! forecast context the assistant answers from.

use crossbeam_channel::{Receiver, TryRecvError};
use egui::text::{LayoutJob, TextFormat};
use egui::{
    vec2, Align, Color32, Context, CornerRadius, FontId, Id, Key, Layout, Margin, Response,
    RichText, ScrollArea, Sense, Stroke, Ui, ViewportBuilder, ViewportClass, ViewportId,
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
    /// Text on the user's own messages.
    pub user_text: Color32,
    /// Text on an accent-filled button.
    pub on_accent: Color32,
    /// "Ready" status dot.
    pub ok: Color32,
    pub corner: u8,
    /// Small caption font (labels, hints).
    pub caption: FontId,
    /// Message text.
    pub body: FontId,
    /// Bold text in replies.
    pub strong: FontId,
    /// Window and empty-state titles.
    pub title: FontId,
}

impl ChatTheme {
    fn strong_small(&self) -> FontId {
        FontId::new(self.caption.size + 1.5, self.strong.family.clone())
    }
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
            .with_inner_size([480.0, 700.0])
            .with_min_inner_size([360.0, 440.0]);
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
                let t = theme;
                let endpoint = settings.endpoint();
                egui::Panel::top("chat_head")
                    .frame(
                        egui::Frame::NONE
                            .fill(t.bg)
                            .inner_margin(Margin::symmetric(18, 14)),
                    )
                    .show_separator_line(false)
                    .resizable(false)
                    .show(ui, |ui| {
                        if let Some(a) = self.header(ui, t, settings, context_label) {
                            action = a;
                        }
                        hairline(ui, t, 14.0);
                    });
                let mut send_now = false;
                egui::Panel::bottom("chat_compose")
                    .frame(egui::Frame::NONE.fill(t.bg).inner_margin(Margin {
                        left: 16,
                        right: 16,
                        top: 8,
                        bottom: 12,
                    }))
                    .show_separator_line(false)
                    .resizable(false)
                    .show(ui, |ui| {
                        send_now = self.composer(ui, t, endpoint.is_some());
                    });
                egui::CentralPanel::default()
                    .frame(egui::Frame::NONE.fill(t.bg))
                    .show(ui, |ui| {
                        if let Some(a) = self.log(ui, t, settings, context_label) {
                            action = a;
                        }
                    });
                if send_now && !self.is_busy() {
                    if let (Some(e), Some(sys)) = (endpoint, system.take()) {
                        let text = std::mem::take(&mut self.input);
                        self.send(&text, e, sys(), rt, client, &vctx);
                        self.focus_input = true;
                    }
                }
            },
        );
        if self.pending.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(60));
        }
        action
    }

    /// Title, model and context, and the window's actions.
    fn header(
        &mut self,
        ui: &mut Ui,
        t: &ChatTheme,
        settings: &AiSettings,
        context_label: &str,
    ) -> Option<ChatAction> {
        let mut action = None;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.label(
                    RichText::new("Assistant")
                        .font(t.title.clone())
                        .color(t.text),
                );
                let (dot, model) = match settings.endpoint() {
                    Some(e) if settings.problem().is_none() => {
                        (t.ok, format!("{} · {}", settings.provider.label(), e.model))
                    }
                    _ => (t.muted, "Not set up".to_string()),
                };
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    let (r, _) = ui.allocate_exact_size(vec2(7.0, 14.0), Sense::hover());
                    ui.painter().circle_filled(r.center(), 3.5, dot);
                    ui.label(
                        RichText::new(format!("{model}  ·  {context_label}"))
                            .font(t.caption.clone())
                            .color(t.muted),
                    );
                });
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if outline_button(ui, t, "Settings")
                    .on_hover_text("AI provider, key and model")
                    .clicked()
                {
                    action = Some(ChatAction::OpenSettings);
                }
                let can_clear = !self.messages.is_empty() || self.error.is_some();
                if ui
                    .add_enabled_ui(can_clear, |ui| outline_button(ui, t, "New chat"))
                    .inner
                    .on_hover_text("Clear this conversation")
                    .clicked()
                {
                    self.clear();
                }
            });
        });
        action
    }

    /// The conversation (or the empty state).
    fn log(
        &mut self,
        ui: &mut Ui,
        t: &ChatTheme,
        settings: &AiSettings,
        context_label: &str,
    ) -> Option<ChatAction> {
        let mut action = None;
        ScrollArea::vertical()
            .id_salt("chat_log")
            .auto_shrink([false, false])
            .stick_to_bottom(true)
            .show(ui, |ui| {
                egui::Frame::NONE
                    .inner_margin(Margin::symmetric(16, 12))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.spacing_mut().item_spacing.y = 12.0;
                        if let Some(problem) = settings.problem() {
                            ui.add_space(40.0);
                            ui.vertical_centered(|ui| {
                                ui.label(RichText::new("✦").size(30.0).color(t.accent));
                                ui.label(
                                    RichText::new("Set up the assistant")
                                        .font(t.title.clone())
                                        .color(t.text),
                                );
                                ui.add(
                                    egui::Label::new(RichText::new(problem).color(t.muted)).wrap(),
                                );
                                ui.add_space(4.0);
                                if filled_button(ui, t, "Open AI settings").clicked() {
                                    action = Some(ChatAction::OpenSettings);
                                }
                            });
                        } else if self.messages.is_empty() {
                            self.empty_state(ui, t, context_label);
                        }
                        for (i, m) in self.messages.iter().enumerate() {
                            bubble(ui, t, m, i);
                        }
                        if self.is_busy() {
                            typing(ui, t);
                        }
                        if let Some(e) = &self.error {
                            notice(ui, t, t.danger, e);
                        }
                        if std::mem::take(&mut self.scroll_to_end) {
                            ui.scroll_to_cursor(Some(Align::BOTTOM));
                        }
                    });
            });
        action
    }

    fn empty_state(&mut self, ui: &mut Ui, t: &ChatTheme, context_label: &str) {
        ui.add_space(24.0);
        ui.vertical_centered(|ui| {
            ui.spacing_mut().item_spacing.y = 6.0;
            ui.label(RichText::new("✦").size(30.0).color(t.accent));
            ui.label(
                RichText::new("Ask about the forecast")
                    .font(t.title.clone())
                    .color(t.text),
            );
            ui.label(
                RichText::new(format!(
                    "Answers use the latest forecast for {context_label}."
                ))
                .font(t.body.clone())
                .color(t.muted),
            );
        });
        ui.add_space(10.0);
        for s in SUGGESTIONS {
            if suggestion(ui, t, s).clicked() {
                self.input = s.to_string();
                self.focus_input = true;
            }
        }
    }

    /// The message box and Send button. Returns true to send.
    fn composer(&mut self, ui: &mut Ui, t: &ChatTheme, ready: bool) -> bool {
        let mut send_now = false;
        let input_id = Id::unique("ss-assistant-input");
        let focused = ui.memory(|m| m.has_focus(input_id));
        let corner = CornerRadius::same(t.corner.min(14));
        egui::Frame::new()
            .fill(t.surface)
            .stroke(Stroke::new(
                if focused { 1.5 } else { 1.0 },
                if focused { t.accent } else { t.border },
            ))
            .corner_radius(corner)
            .inner_margin(Margin {
                left: 12,
                right: 8,
                top: 8,
                bottom: 8,
            })
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let send_w = 72.0;
                    let w = (ui.available_width() - send_w - 8.0).max(80.0);
                    let edit = egui::TextEdit::multiline(&mut self.input)
                        .id(input_id)
                        .frame(egui::Frame::NONE)
                        .font(t.body.clone())
                        .text_color(t.text)
                        .hint_text(
                            RichText::new(if ready {
                                "Ask about the weather…"
                            } else {
                                "Set up AI to start chatting"
                            })
                            .font(t.body.clone())
                            .color(t.muted),
                        )
                        .desired_rows(2)
                        .desired_width(w)
                        .lock_focus(true);
                    let r = ScrollArea::vertical()
                        .id_salt("chat_input_scroll")
                        .max_height(140.0)
                        .max_width(w)
                        .show(ui, |ui| ui.add_enabled(ready, edit))
                        .inner;
                    if std::mem::take(&mut self.focus_input) {
                        r.request_focus();
                    }
                    if r.has_focus()
                        && ui.input(|i| i.key_pressed(Key::Enter) && !i.modifiers.shift)
                    {
                        // Drop the newline the key just inserted.
                        while self.input.ends_with('\n') {
                            self.input.pop();
                        }
                        send_now = true;
                    }
                    let can_send = ready && !self.is_busy() && !self.input.trim().is_empty();
                    if ui
                        .add_enabled_ui(can_send, |ui| {
                            filled_button_sized(ui, t, "Send", vec2(send_w, 34.0))
                        })
                        .inner
                        .on_hover_text("Send (Enter)")
                        .clicked()
                    {
                        send_now = true;
                    }
                });
            });
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            ui.label(
                RichText::new("Enter to send · Shift+Enter for a new line.  ")
                    .font(t.caption.clone())
                    .color(t.muted),
            );
        });
        ui.add(
            egui::Label::new(
                RichText::new(AI_DISCLAIMER)
                    .font(t.caption.clone())
                    .color(t.muted.gamma_multiply(0.85)),
            )
            .wrap(),
        );
        send_now
    }
}

/// A thin rule across the panel, `gap` below the content.
fn hairline(ui: &mut Ui, t: &ChatTheme, gap: f32) {
    let r = ui.max_rect();
    let y = ui.min_rect().bottom() + gap;
    ui.painter().hline(
        (r.left() - 18.0)..=(r.right() + 18.0),
        y,
        Stroke::new(1.0, t.border),
    );
}

fn button_scope(
    ui: &mut Ui,
    t: &ChatTheme,
    fill: Color32,
    stroke: Stroke,
    add: impl FnOnce(&mut Ui) -> Response,
) -> Response {
    ui.scope(|ui| {
        let corner = CornerRadius::same(t.corner.min(10));
        let w = &mut ui.visuals_mut().widgets;
        w.inactive.weak_bg_fill = fill;
        w.inactive.bg_stroke = stroke;
        for s in [&mut w.inactive, &mut w.hovered, &mut w.active] {
            s.corner_radius = corner;
        }
        w.hovered.weak_bg_fill = blend(fill, t.accent, 0.18);
        w.hovered.bg_stroke = Stroke::new(1.0, t.accent);
        add(ui)
    })
    .inner
}

fn outline_button(ui: &mut Ui, t: &ChatTheme, text: &str) -> Response {
    button_scope(ui, t, t.bg, Stroke::new(1.0, t.border), |ui| {
        ui.add(
            egui::Button::new(RichText::new(text).font(t.strong_small()).color(t.text))
                .min_size(vec2(0.0, 30.0)),
        )
    })
}

fn filled_button(ui: &mut Ui, t: &ChatTheme, text: &str) -> Response {
    filled_button_sized(ui, t, text, vec2(0.0, 32.0))
}

fn filled_button_sized(ui: &mut Ui, t: &ChatTheme, text: &str, size: egui::Vec2) -> Response {
    button_scope(ui, t, t.accent, Stroke::new(1.0, t.accent), |ui| {
        let w = &mut ui.visuals_mut().widgets;
        w.hovered.weak_bg_fill = blend(t.accent, Color32::WHITE, 0.12);
        ui.add(
            egui::Button::new(
                RichText::new(text)
                    .font(t.strong_small())
                    .color(t.on_accent),
            )
            .min_size(size),
        )
    })
}

fn blend(a: Color32, b: Color32, k: f32) -> Color32 {
    let l = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * k).round() as u8;
    Color32::from_rgba_unmultiplied(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()), a.a())
}

/// A starter question: a full-width outlined card.
fn suggestion(ui: &mut Ui, t: &ChatTheme, text: &str) -> Response {
    let corner = CornerRadius::same(t.corner.min(12));
    let id = ui.next_auto_id();
    let hovered = ui.ctx().read_response(id).is_some_and(|r| r.hovered());
    let r = egui::Frame::new()
        .fill(if hovered {
            blend(t.surface, t.accent, 0.08)
        } else {
            t.surface
        })
        .stroke(Stroke::new(1.0, if hovered { t.accent } else { t.border }))
        .corner_radius(corner)
        .inner_margin(Margin::symmetric(14, 10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.add(
                    egui::Label::new(RichText::new(text).font(t.body.clone()).color(t.text))
                        .wrap()
                        .selectable(false),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(RichText::new("→").font(t.body.clone()).color(t.accent));
                });
            });
        })
        .response;
    let r = ui.interact(r.rect, id, Sense::click());
    if r.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    r
}

/// An error or notice in the conversation.
fn notice(ui: &mut Ui, t: &ChatTheme, color: Color32, text: &str) {
    egui::Frame::new()
        .fill(Color32::from_rgba_unmultiplied(
            color.r(),
            color.g(),
            color.b(),
            26,
        ))
        .stroke(Stroke::new(1.0, color.gamma_multiply(0.6)))
        .corner_radius(CornerRadius::same(t.corner.min(10)))
        .inner_margin(Margin::symmetric(12, 9))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.add(egui::Label::new(RichText::new(text).font(t.body.clone()).color(color)).wrap());
        });
}

/// The assistant's "typing" bubble: three pulsing dots.
fn typing(ui: &mut Ui, t: &ChatTheme) {
    let time = ui.input(|i| i.time);
    egui::Frame::new()
        .fill(t.surface)
        .stroke(Stroke::new(1.0, t.border))
        .corner_radius(CornerRadius::same(t.corner.min(14)))
        .inner_margin(Margin::symmetric(14, 12))
        .show(ui, |ui| {
            let (r, _) = ui.allocate_exact_size(vec2(36.0, 10.0), Sense::hover());
            for i in 0..3 {
                let phase = (time * 4.0 - f64::from(i) * 0.6).sin() as f32 * 0.5 + 0.5;
                ui.painter().circle_filled(
                    egui::pos2(r.left() + 6.0 + 12.0 * i as f32, r.center().y),
                    3.0 + phase,
                    t.accent.gamma_multiply(0.45 + 0.55 * phase),
                );
            }
        });
}

/// Light markdown for replies: `**bold**`, `# headings`, `- bullets`,
/// `1. lists` and `` `code` `` (shown in the accent colour).
pub fn markdown_job(text: &str, t: &ChatTheme, wrap: f32) -> LayoutJob {
    let mut job = LayoutJob::default();
    job.wrap.max_width = wrap;
    let body = TextFormat::simple(t.body.clone(), t.text);
    let strong = TextFormat::simple(t.strong.clone(), t.text);
    let code = TextFormat::simple(t.body.clone(), t.accent);
    let lines: Vec<&str> = text.lines().collect();
    for (n, raw) in lines.iter().enumerate() {
        let line = raw.trim_end();
        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();
        let (prefix, rest, heading) = if let Some(h) = trimmed
            .strip_prefix("### ")
            .or_else(|| trimmed.strip_prefix("## "))
            .or_else(|| trimmed.strip_prefix("# "))
        {
            (String::new(), h, true)
        } else if let Some(b) = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "))
            .or_else(|| trimmed.strip_prefix("• "))
        {
            (format!("{}•  ", " ".repeat(indent.min(6) + 1)), b, false)
        } else {
            (String::new(), trimmed, false)
        };
        if !prefix.is_empty() {
            job.append(&prefix, 0.0, TextFormat::simple(t.body.clone(), t.accent));
        } else if indent > 0 {
            job.append(&" ".repeat(indent.min(6)), 0.0, body.clone());
        }
        // Inline: **bold** and `code`.
        let mut bold = heading;
        let mut s = rest;
        while !s.is_empty() {
            let next_b = s.find("**");
            let next_c = s.find('`');
            match (next_b, next_c) {
                (Some(b), c) if c.is_none_or(|c| b < c) => {
                    if b > 0 {
                        job.append(
                            &s[..b],
                            0.0,
                            if bold { strong.clone() } else { body.clone() },
                        );
                    }
                    bold = !bold;
                    s = &s[b + 2..];
                }
                (_, Some(c)) => {
                    if c > 0 {
                        job.append(
                            &s[..c],
                            0.0,
                            if bold { strong.clone() } else { body.clone() },
                        );
                    }
                    let after = &s[c + 1..];
                    match after.find('`') {
                        Some(e) => {
                            job.append(&after[..e], 0.0, code.clone());
                            s = &after[e + 1..];
                        }
                        None => {
                            job.append(&s[c..], 0.0, body.clone());
                            s = "";
                        }
                    }
                }
                _ => {
                    job.append(s, 0.0, if bold { strong.clone() } else { body.clone() });
                    s = "";
                }
            }
        }
        if n + 1 < lines.len() {
            job.append("\n", 0.0, body.clone());
        }
    }
    job
}

fn bubble(ui: &mut Ui, t: &ChatTheme, m: &Message, index: usize) {
    let user = m.role == Role::User;
    let max_w = (ui.available_width() * 0.86).max(160.0);
    let layout = Layout::top_down(if user { Align::Max } else { Align::Min });
    let big = CornerRadius::same(t.corner.min(14));
    // A tail corner on the speaker's side.
    let corner = if user {
        CornerRadius { ne: 3, ..big }
    } else {
        CornerRadius { nw: 3, ..big }
    };
    ui.with_layout(layout, |ui| {
        {
            ui.spacing_mut().item_spacing.y = 4.0;
            ui.with_layout(
                if user {
                    Layout::right_to_left(Align::Min)
                } else {
                    Layout::left_to_right(Align::Min)
                },
                |ui| {
                    ui.label(
                        RichText::new(if user { "You" } else { "✦ Assistant" })
                            .font(t.caption.clone())
                            .color(if user { t.muted } else { t.accent }),
                    );
                },
            );
            let frame = egui::Frame::new()
                .fill(if user { t.user_bubble } else { t.surface })
                .stroke(Stroke::new(
                    1.0,
                    if user { Color32::TRANSPARENT } else { t.border },
                ))
                .corner_radius(corner)
                .inner_margin(Margin::symmetric(14, 10))
                .show(ui, |ui| {
                    ui.set_max_width(max_w);
                    if user {
                        ui.add(
                            egui::Label::new(
                                RichText::new(&m.text)
                                    .font(t.body.clone())
                                    .color(t.user_text),
                            )
                            .wrap(),
                        );
                    } else {
                        let job = markdown_job(&m.text, t, max_w);
                        ui.add(egui::Label::new(job).wrap());
                    }
                });
            // Copy appears under a message while it is hovered.
            let hover_id = Id::unique(("chat-msg-hover", index));
            let hovered = frame.response.contains_pointer()
                || ui
                    .ctx()
                    .data(|d| d.get_temp::<bool>(hover_id).unwrap_or(false));
            let row = ui.with_layout(
                if user {
                    Layout::right_to_left(Align::Min)
                } else {
                    Layout::left_to_right(Align::Min)
                },
                |ui| {
                    ui.scope(|ui| {
                        if !hovered {
                            ui.set_invisible();
                        }
                        let r = ui.add(
                            egui::Button::new(
                                RichText::new("Copy").font(t.caption.clone()).color(t.muted),
                            )
                            .frame(false),
                        );
                        if r.clicked() {
                            ui.ctx().copy_text(m.text.clone());
                        }
                        r.hovered()
                    })
                    .inner
                },
            );
            ui.ctx()
                .data_mut(|d| d.insert_temp(hover_id, row.inner && hovered));
        }
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

    fn theme() -> ChatTheme {
        ChatTheme {
            bg: Color32::BLACK,
            surface: Color32::BLACK,
            border: Color32::GRAY,
            text: Color32::WHITE,
            muted: Color32::GRAY,
            accent: Color32::YELLOW,
            user_bubble: Color32::BLUE,
            danger: Color32::RED,
            user_text: Color32::WHITE,
            on_accent: Color32::BLACK,
            ok: Color32::GREEN,
            corner: 6,
            caption: FontId::proportional(12.0),
            body: FontId::proportional(14.0),
            strong: FontId::monospace(14.0),
            title: FontId::proportional(20.0),
        }
    }

    #[test]
    fn markdown_strips_markers_and_styles_spans() {
        let t = theme();
        let job = markdown_job("## Today\n- **Rain** by `3 PM`\nplain", &t, 300.0);
        assert_eq!(job.text, "Today\n •  Rain by 3 PM\nplain");
        let fmt_of = |needle: &str| {
            let at = job.text.find(needle).unwrap();
            job.sections
                .iter()
                .find(|s| s.byte_range.contains(&egui::text::ByteIndex(at)))
                .unwrap()
                .format
                .clone()
        };
        assert_eq!(fmt_of("Today").font_id, t.strong, "headings are bold");
        assert_eq!(fmt_of("Rain").font_id, t.strong);
        assert_eq!(fmt_of("by").font_id, t.body);
        assert_eq!(fmt_of("3 PM").color, t.accent, "code in the accent");
        assert_eq!(fmt_of("•").color, t.accent);
    }

    #[test]
    fn unbalanced_markdown_keeps_the_text() {
        let job = markdown_job("a `b", &theme(), 300.0);
        assert_eq!(job.text, "a `b");
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
