//! Settings that only the simple app has, kept in `simple.json` next to the
//! shared `config.json` / `locations.json`. AI provider settings live in the
//! shared config (`Config::ai`); older `simple.json` files that held a Claude
//! key are migrated into it once.

use serde::{Deserialize, Serialize};
use ss_core::ai::{AiProvider, AiSettings};
use ss_store::Store;

pub const PREFS_FILE: &str = "simple.json";

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Prefs {
    /// Ask the AI for a summary automatically whenever a forecast loads
    /// (otherwise only on request).
    pub ai_auto_summary: bool,
    /// Turn off entrance, morph and transition animations.
    pub reduce_motion: bool,
    // --- legacy (pre-multi-provider) fields, read once for migration ---
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ai_enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub anthropic_api_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ai_model: Option<String>,
}

impl Prefs {
    pub fn load(store: &Store) -> Prefs {
        store.read(PREFS_FILE)
    }

    pub fn save(&self, store: &Store) {
        if let Err(e) = store.write(PREFS_FILE, self) {
            tracing::warn!("could not save {PREFS_FILE}: {e}");
        }
    }

    /// Move legacy Claude settings into `ai` when it hasn't been set up yet.
    /// Returns true when anything changed (both files should be saved).
    pub fn migrate_into(&mut self, ai: &mut AiSettings) -> bool {
        let had_legacy = self.ai_enabled.is_some()
            || self.anthropic_api_key.is_some()
            || self.ai_model.is_some();
        if !had_legacy {
            return false;
        }
        if *ai == AiSettings::default() {
            *ai = AiSettings {
                enabled: self.ai_enabled.unwrap_or(false),
                provider: AiProvider::Anthropic,
                base_url: String::new(),
                api_key: self.anthropic_api_key.clone(),
                model: self.ai_model.clone().unwrap_or_default(),
            };
        }
        self.ai_enabled = None;
        self.anthropic_api_key = None;
        self.ai_model = None;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_claude_settings_migrate_once() {
        let mut p: Prefs = serde_json::from_str(
            r#"{"aiEnabled":true,"anthropicApiKey":"sk-x","aiModel":"claude-opus-5-5","aiAutoSummary":true}"#,
        )
        .unwrap();
        let mut ai = AiSettings::default();
        assert!(p.migrate_into(&mut ai));
        assert!(ai.enabled && ai.provider == AiProvider::Anthropic);
        assert_eq!(ai.key().as_deref(), Some("sk-x"));
        assert!(p.ai_auto_summary);
        assert!(!p.migrate_into(&mut ai), "nothing left to migrate");
        let saved = serde_json::to_string(&p).unwrap();
        assert!(!saved.contains("anthropicApiKey"));
    }

    #[test]
    fn migration_never_overwrites_configured_ai() {
        let mut p = Prefs {
            anthropic_api_key: Some("old".into()),
            ..Default::default()
        };
        let mut ai = AiSettings {
            enabled: true,
            provider: AiProvider::Ollama,
            model: "llama3.2".into(),
            ..Default::default()
        };
        let before = ai.clone();
        assert!(p.migrate_into(&mut ai));
        assert_eq!(ai, before);
    }
}
