//! Settings that only the simple app has, kept in `simple.json` next to the
//! shared `config.json` / `locations.json`.

use serde::{Deserialize, Serialize};
use ss_store::Store;

pub const PREFS_FILE: &str = "simple.json";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Prefs {
    /// AI summaries and answers. Off by default; needs an API key.
    pub ai_enabled: bool,
    pub anthropic_api_key: Option<String>,
    pub ai_model: String,
    /// Ask the AI for a summary automatically whenever a forecast loads
    /// (otherwise only on request).
    pub ai_auto_summary: bool,
}

impl Default for Prefs {
    fn default() -> Self {
        Prefs {
            ai_enabled: false,
            anthropic_api_key: None,
            ai_model: crate::ai::DEFAULT_MODEL.to_string(),
            ai_auto_summary: false,
        }
    }
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

    /// The key, when AI is on and one is set.
    pub fn ai_key(&self) -> Option<&str> {
        if !self.ai_enabled {
            return None;
        }
        self.anthropic_api_key
            .as_deref()
            .map(str::trim)
            .filter(|k| !k.is_empty())
    }

    pub fn model(&self) -> &str {
        let m = self.ai_model.trim();
        if m.is_empty() {
            crate::ai::DEFAULT_MODEL
        } else {
            m
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ai_key_requires_enabled_and_nonempty() {
        let mut p = Prefs::default();
        assert_eq!(p.ai_key(), None);
        p.anthropic_api_key = Some("  sk-test  ".into());
        assert_eq!(p.ai_key(), None);
        p.ai_enabled = true;
        assert_eq!(p.ai_key(), Some("sk-test"));
        p.anthropic_api_key = Some("   ".into());
        assert_eq!(p.ai_key(), None);
    }

    #[test]
    fn roundtrips_and_tolerates_missing_fields() {
        let p: Prefs = serde_json::from_str(r#"{"aiEnabled":true}"#).unwrap();
        assert!(p.ai_enabled);
        assert_eq!(p.model(), crate::ai::DEFAULT_MODEL);
        let s = serde_json::to_string(&p).unwrap();
        assert_eq!(serde_json::from_str::<Prefs>(&s).unwrap(), p);
    }
}
