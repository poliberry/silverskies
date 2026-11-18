//! AI provider settings shared by both apps (the network client lives in
//! the `ss-ai` crate). Claude is reached through Anthropic's own Messages
//! API; everything else through the OpenAI-compatible chat-completions API
//! that most providers and local runners expose.

use serde::{Deserialize, Serialize};

/// The default Claude model.
pub const DEFAULT_CLAUDE_MODEL: &str = "claude-opus-5-5";

/// The wire protocol a provider speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    /// Anthropic Messages API (`POST /v1/messages`).
    Anthropic,
    /// OpenAI-compatible chat completions (`POST {base}/chat/completions`).
    OpenAiCompatible,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AiProvider {
    /// Claude, via Anthropic's API.
    #[default]
    Anthropic,
    /// OpenAI's API (GPT and Codex models).
    OpenAi,
    /// OpenRouter: one key for many providers' models.
    OpenRouter,
    /// A model running locally with Ollama. No key needed.
    Ollama,
    /// OpenCode Go: the OpenCode subscription, through its OpenAI-compatible
    /// API, with a Console API key.
    #[serde(rename = "opencode-go")]
    OpenCodeGo,
    /// OpenCode Console pay-as-you-go (Zen), same API shape as Go.
    #[serde(rename = "opencode-console")]
    OpenCodeConsole,
    /// Any other OpenAI-compatible endpoint (LM Studio, vLLM, a company
    /// gateway…): you supply the base URL.
    Custom,
}

impl AiProvider {
    pub const ALL: [AiProvider; 7] = [
        AiProvider::Anthropic,
        AiProvider::OpenAi,
        AiProvider::OpenRouter,
        AiProvider::OpenCodeGo,
        AiProvider::OpenCodeConsole,
        AiProvider::Ollama,
        AiProvider::Custom,
    ];

    pub fn label(self) -> &'static str {
        match self {
            AiProvider::Anthropic => "Claude (Anthropic)",
            AiProvider::OpenAi => "OpenAI",
            AiProvider::OpenRouter => "OpenRouter",
            AiProvider::OpenCodeGo => "OpenCode Go",
            AiProvider::OpenCodeConsole => "OpenCode Console (pay-as-you-go)",
            AiProvider::Ollama => "Ollama (local)",
            AiProvider::Custom => "Other OpenAI-compatible",
        }
    }

    pub fn protocol(self) -> Protocol {
        match self {
            AiProvider::Anthropic => Protocol::Anthropic,
            _ => Protocol::OpenAiCompatible,
        }
    }

    /// Base URL used when the settings leave it blank.
    pub fn default_base_url(self) -> &'static str {
        match self {
            AiProvider::Anthropic => "https://api.anthropic.com",
            AiProvider::OpenAi => "https://api.openai.com/v1",
            AiProvider::OpenRouter => "https://openrouter.ai/api/v1",
            AiProvider::OpenCodeGo => "https://opencode.ai/inference/go/openai/v1",
            AiProvider::OpenCodeConsole => "https://opencode.ai/inference/openai/v1",
            AiProvider::Ollama => "http://localhost:11434/v1",
            AiProvider::Custom => "",
        }
    }

    /// Model used when the settings leave it blank (only Claude has one;
    /// other providers' model names are the user's choice).
    pub fn default_model(self) -> &'static str {
        match self {
            AiProvider::Anthropic => DEFAULT_CLAUDE_MODEL,
            _ => "",
        }
    }

    /// OpenCode's inference API (Go and pay-as-you-go), which wants a
    /// stable `x-opencode-session` per conversation.
    pub fn is_opencode(self) -> bool {
        matches!(self, AiProvider::OpenCodeGo | AiProvider::OpenCodeConsole)
    }

    /// A short provider-specific note for the settings form.
    pub fn note(self) -> Option<&'static str> {
        match self {
            AiProvider::OpenCodeGo => Some(
                "Use an API key from the OpenCode Console with a Go subscription, and a Go \
                 model ID (see opencode.ai/console/go). OpenCode Go is designed for coding \
                 agents, so check that its usage terms allow use from a weather app.",
            ),
            AiProvider::OpenCodeConsole => Some(
                "Use an API key from the OpenCode Console (pay-as-you-go) and a paid model \
                 ID. Free models only work inside OpenCode itself.",
            ),
            _ => None,
        }
    }

    pub fn needs_key(self) -> bool {
        !matches!(self, AiProvider::Ollama | AiProvider::Custom)
    }

    /// Example model names for the settings hint.
    pub fn model_hint(self) -> &'static str {
        match self {
            AiProvider::Anthropic => DEFAULT_CLAUDE_MODEL,
            AiProvider::OpenAi => "the model ID from your OpenAI account",
            AiProvider::OpenRouter => "e.g. anthropic/claude-opus-5-5",
            AiProvider::OpenCodeGo | AiProvider::OpenCodeConsole => "e.g. kimi-k3",
            AiProvider::Ollama => "e.g. llama3.2",
            AiProvider::Custom => "the model name your endpoint expects",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AiSettings {
    /// Off by default: nothing is sent anywhere until the user turns it on.
    pub enabled: bool,
    pub provider: AiProvider,
    /// Blank = the provider's default.
    pub base_url: String,
    pub api_key: Option<String>,
    /// Blank = the provider's default (Claude only).
    pub model: String,
}

/// Everything needed to make a request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AiEndpoint {
    pub protocol: Protocol,
    pub base_url: String,
    pub api_key: Option<String>,
    pub model: String,
    pub provider: AiProvider,
    /// Sent as the user agent (`<app>/<version>`), so providers can tell
    /// which client is calling, as OpenCode asks.
    pub app: String,
    /// A stable id for one conversation (OpenCode's `x-opencode-session`);
    /// one-off requests get a fresh id when this is `None`.
    pub session: Option<String>,
}

impl AiEndpoint {
    /// Name the calling app, e.g. "Silver Skies Pro" → `silver-skies-pro`.
    pub fn for_app(mut self, name: &str) -> Self {
        self.app = name
            .to_lowercase()
            .split(|c: char| !c.is_ascii_alphanumeric())
            .filter(|w| !w.is_empty())
            .collect::<Vec<_>>()
            .join("-");
        self
    }

    /// Tie this request to a conversation.
    pub fn in_session(mut self, id: &str) -> Self {
        self.session = Some(id.to_string());
        self
    }
}

impl AiSettings {
    pub fn base_url(&self) -> String {
        let b = self.base_url.trim();
        let b = if b.is_empty() {
            self.provider.default_base_url()
        } else {
            b
        };
        b.trim_end_matches('/').to_string()
    }

    pub fn model(&self) -> String {
        let m = self.model.trim();
        if m.is_empty() {
            self.provider.default_model().to_string()
        } else {
            m.to_string()
        }
    }

    pub fn key(&self) -> Option<String> {
        self.api_key
            .as_deref()
            .map(str::trim)
            .filter(|k| !k.is_empty())
            .map(str::to_string)
    }

    /// Why AI can't be used yet, if it can't (for the settings and chat UI).
    pub fn problem(&self) -> Option<&'static str> {
        if !self.enabled {
            Some("AI is turned off. Turn it on in Settings → AI.")
        } else if self.provider.needs_key() && self.key().is_none() {
            Some("Add an API key in Settings → AI.")
        } else if self.base_url().is_empty() {
            Some("Add the provider's base URL in Settings → AI.")
        } else if self.model().is_empty() {
            Some("Choose a model in Settings → AI.")
        } else {
            None
        }
    }

    /// The endpoint to call, when AI is on and fully configured.
    pub fn endpoint(&self) -> Option<AiEndpoint> {
        if self.problem().is_some() {
            return None;
        }
        Some(AiEndpoint {
            protocol: self.provider.protocol(),
            base_url: self.base_url(),
            api_key: self.key(),
            model: self.model(),
            provider: self.provider,
            app: "silver-skies".to_string(),
            session: None,
        })
    }
}

/// Shown wherever AI output appears.
pub const AI_DISCLAIMER: &str = "AI can make mistakes. Check important details against \
official forecasts, and during severe weather follow warnings from your national weather \
service and local authorities.";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opencode_providers_have_endpoints_and_need_a_key() {
        for (p, url) in [
            (
                AiProvider::OpenCodeGo,
                "https://opencode.ai/inference/go/openai/v1",
            ),
            (
                AiProvider::OpenCodeConsole,
                "https://opencode.ai/inference/openai/v1",
            ),
        ] {
            let mut s = AiSettings {
                enabled: true,
                provider: p,
                model: "kimi-k3".into(),
                ..Default::default()
            };
            assert!(s.problem().unwrap().contains("API key"));
            s.api_key = Some("oc_sk_test".into());
            let e = s.endpoint().unwrap();
            assert_eq!(e.base_url, url);
            assert_eq!(e.protocol, Protocol::OpenAiCompatible);
            assert!(p.is_opencode() && p.note().is_some());
        }
        let json = serde_json::to_string(&AiProvider::OpenCodeGo).unwrap();
        assert_eq!(json, "\"opencode-go\"");
    }

    #[test]
    fn endpoint_names_the_app_and_session() {
        let s = AiSettings {
            enabled: true,
            api_key: Some("k".into()),
            ..Default::default()
        };
        let e = s
            .endpoint()
            .unwrap()
            .for_app("Silver Skies Pro")
            .in_session("ses_1");
        assert_eq!(e.app, "silver-skies-pro");
        assert_eq!(e.session.as_deref(), Some("ses_1"));
    }

    #[test]
    fn off_by_default_and_explains_why() {
        let s = AiSettings::default();
        assert!(s.endpoint().is_none());
        assert!(s.problem().unwrap().contains("turned off"));
    }

    #[test]
    fn claude_needs_a_key_and_has_a_default_model() {
        let mut s = AiSettings {
            enabled: true,
            ..Default::default()
        };
        assert!(s.problem().unwrap().contains("API key"));
        s.api_key = Some(" sk-test ".into());
        let e = s.endpoint().unwrap();
        assert_eq!(e.protocol, Protocol::Anthropic);
        assert_eq!(e.model, DEFAULT_CLAUDE_MODEL);
        assert_eq!(e.api_key.as_deref(), Some("sk-test"));
        assert_eq!(e.base_url, "https://api.anthropic.com");
    }

    #[test]
    fn ollama_needs_no_key_but_a_model() {
        let mut s = AiSettings {
            enabled: true,
            provider: AiProvider::Ollama,
            ..Default::default()
        };
        assert!(s.problem().unwrap().contains("model"));
        s.model = "llama3.2".into();
        let e = s.endpoint().unwrap();
        assert_eq!(e.protocol, Protocol::OpenAiCompatible);
        assert_eq!(e.base_url, "http://localhost:11434/v1");
        assert_eq!(e.api_key, None);
    }

    #[test]
    fn custom_needs_a_base_url_and_trims_slashes() {
        let mut s = AiSettings {
            enabled: true,
            provider: AiProvider::Custom,
            model: "m".into(),
            ..Default::default()
        };
        assert!(s.problem().unwrap().contains("base URL"));
        s.base_url = "https://gateway.example/v1/".into();
        assert_eq!(s.endpoint().unwrap().base_url, "https://gateway.example/v1");
    }

    #[test]
    fn serde_roundtrip() {
        let s = AiSettings {
            enabled: true,
            provider: AiProvider::OpenRouter,
            model: "x/y".into(),
            ..Default::default()
        };
        let j = serde_json::to_string(&s).unwrap();
        assert!(j.contains("\"provider\":\"open-router\""));
        assert_eq!(serde_json::from_str::<AiSettings>(&j).unwrap(), s);
        assert_eq!(
            serde_json::from_str::<AiSettings>("{}").unwrap(),
            AiSettings::default()
        );
    }
}
