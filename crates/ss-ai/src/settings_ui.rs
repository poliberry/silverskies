//! The AI settings form, shared by both apps' settings windows.

use egui::{Color32, RichText, Ui};
use ss_core::ai::{AiProvider, AiSettings, AI_DISCLAIMER};

/// Draw the form; returns true when anything changed (save the config).
pub fn ai_settings_form(ui: &mut Ui, ai: &mut AiSettings, muted: Color32, warn: Color32) -> bool {
    let mut changed = false;
    ui.label(
        RichText::new(
            "Optional. Powers AI forecast summaries, answers and the assistant chat, using your \
             own account with an AI provider: Claude, OpenAI, OpenRouter, a local model in \
             Ollama, or any other service with an OpenAI-compatible API (for example OpenCode, \
             LM Studio or a company gateway). The forecasts for your saved places are sent with \
             each request; nothing else is.",
        )
        .size(12.5)
        .color(muted),
    );
    changed |= ui.checkbox(&mut ai.enabled, "Turn on AI").changed();
    ui.add_enabled_ui(ai.enabled, |ui| {
        egui::Grid::new("ai_settings_grid")
            .num_columns(2)
            .spacing([16.0, 10.0])
            .show(ui, |ui| {
                ui.label(RichText::new("Provider").color(muted));
                egui::ComboBox::from_id_salt("ai_provider")
                    .selected_text(ai.provider.label())
                    .width(240.0)
                    .show_ui(ui, |ui| {
                        for p in AiProvider::ALL {
                            if ui
                                .selectable_value(&mut ai.provider, p, p.label())
                                .changed()
                            {
                                // Defaults follow the provider unless overridden.
                                ai.base_url.clear();
                                ai.model.clear();
                                changed = true;
                            }
                        }
                    });
                ui.end_row();

                ui.label(RichText::new("Base URL").color(muted));
                let hint = match ai.provider {
                    AiProvider::Custom => "https://…/v1".to_string(),
                    p => p.default_base_url().to_string(),
                };
                changed |= ui
                    .add(
                        egui::TextEdit::singleline(&mut ai.base_url)
                            .hint_text(hint)
                            .desired_width(260.0),
                    )
                    .on_hover_text("Leave blank to use the provider's standard address.")
                    .changed();
                ui.end_row();

                ui.label(RichText::new("API key").color(muted));
                // Edit a copy so merely showing the form never turns "no key"
                // into an empty key (which would mark the config as changed).
                let mut key = ai.api_key.clone().unwrap_or_default();
                let key_hint = if ai.provider.needs_key() {
                    "required"
                } else {
                    "optional"
                };
                if ui
                    .add(
                        egui::TextEdit::singleline(&mut key)
                            .password(true)
                            .hint_text(key_hint)
                            .desired_width(260.0),
                    )
                    .changed()
                {
                    ai.api_key = (!key.is_empty()).then_some(key);
                    changed = true;
                }
                ui.end_row();

                ui.label(RichText::new("Model").color(muted));
                changed |= ui
                    .add(
                        egui::TextEdit::singleline(&mut ai.model)
                            .hint_text(ai.provider.model_hint())
                            .desired_width(260.0),
                    )
                    .changed();
                ui.end_row();
            });
    });
    if ai.enabled {
        if let Some(problem) = ai.problem() {
            ui.label(RichText::new(problem).color(warn));
        }
    }
    ui.label(
        RichText::new(format!("⚠ {AI_DISCLAIMER}"))
            .size(12.0)
            .color(muted),
    );
    changed
}
