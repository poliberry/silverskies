//! Silver Skies AI.
//!
//! * [`complete`] sends a conversation to the configured provider: Claude
//!   through Anthropic's Messages API (there is no official Anthropic SDK
//!   for Rust, so this is plain HTTP), every other provider through the
//!   OpenAI-compatible chat-completions API.
//! * [`digest`] turns a forecast into compact text for prompts.
//! * [`chat`] is the assistant chat window both apps open.
//!
//! Request building and response parsing are pure functions, unit tested
//! without a network.

pub mod chat;
pub mod digest;
pub mod settings_ui;

use serde_json::{json, Value};
use ss_core::ai::{AiEndpoint, AiProvider, Protocol};

pub use ss_core::ai::{AiSettings, AI_DISCLAIMER};

pub const ANTHROPIC_VERSION: &str = "2026-10-21";
/// Lets Anthropic re-run a declined request on a fallback model, routed by
/// refusal category, inside the same call.
pub const ANTHROPIC_FALLBACK_BETA: &str = "server-side-fallback-2026-07-01";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    User,
    Assistant,
}

impl Role {
    fn as_str(self) -> &'static str {
        match self {
            Role::User => "user",
            Role::Assistant => "assistant",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub role: Role,
    pub text: String,
}

impl Message {
    pub fn user(text: impl Into<String>) -> Self {
        Message {
            role: Role::User,
            text: text.into(),
        }
    }
    pub fn assistant(text: impl Into<String>) -> Self {
        Message {
            role: Role::Assistant,
            text: text.into(),
        }
    }
}

/// A ready-to-send HTTP request.
#[derive(Debug, Clone, PartialEq)]
pub struct HttpRequest {
    pub url: String,
    pub headers: Vec<(&'static str, String)>,
    pub body: Value,
}

/// Build the request for `endpoint` with a system prompt and conversation.
pub fn build_request(endpoint: &AiEndpoint, system: &str, messages: &[Message]) -> HttpRequest {
    match endpoint.protocol {
        Protocol::Anthropic => {
            let msgs: Vec<Value> = messages
                .iter()
                .map(|m| json!({ "role": m.role.as_str(), "content": m.text }))
                .collect();
            let mut headers = vec![
                ("anthropic-version", ANTHROPIC_VERSION.to_string()),
                ("anthropic-beta", ANTHROPIC_FALLBACK_BETA.to_string()),
                ("content-type", "application/json".to_string()),
            ];
            if let Some(k) = &endpoint.api_key {
                headers.push(("x-api-key", k.clone()));
            }
            HttpRequest {
                url: format!("{}/v1/messages", endpoint.base_url),
                headers,
                body: json!({
                    "model": endpoint.model,
                    "max_tokens": 16000,
                    // Short weather answers: low effort keeps them quick.
                    "output_config": { "effort": "low" },
                    "fallbacks": "default",
                    "system": system,
                    "messages": msgs,
                }),
            }
        }
        Protocol::OpenAiCompatible => {
            let mut msgs = vec![json!({ "role": "system", "content": system })];
            msgs.extend(
                messages
                    .iter()
                    .map(|m| json!({ "role": m.role.as_str(), "content": m.text })),
            );
            let mut headers = vec![("content-type", "application/json".to_string())];
            if let Some(k) = &endpoint.api_key {
                headers.push(("authorization", format!("Bearer {k}")));
            }
            if endpoint.provider == AiProvider::OpenRouter {
                // OpenRouter's optional app attribution.
                headers.push(("x-title", "Silver Skies".to_string()));
            }
            HttpRequest {
                url: format!("{}/chat/completions", endpoint.base_url),
                headers,
                body: json!({ "model": endpoint.model, "messages": msgs }),
            }
        }
    }
}

fn error_message(v: &Value) -> Option<String> {
    v.pointer("/error/message")
        .and_then(Value::as_str)
        .or_else(|| v.get("error").and_then(Value::as_str))
        .or_else(|| v.get("message").and_then(Value::as_str))
        .map(str::to_string)
}

/// Extract the reply text from a response body.
pub fn parse_response(protocol: Protocol, status: u16, body: &str) -> Result<String, String> {
    let v: Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(_) => {
            return Err(format!(
                "The AI service returned an unreadable response (HTTP {status})."
            ))
        }
    };
    if !(200..300).contains(&status) {
        let msg = error_message(&v).unwrap_or_else(|| "unknown error".into());
        return Err(match status {
            401 | 403 => "The API key was rejected. Check it in Settings → AI.".into(),
            404 => format!("The model or endpoint wasn't found. Check Settings → AI. ({msg})"),
            429 => "The AI service is busy or your quota is used up. Try again shortly.".into(),
            _ => format!("The AI request failed (HTTP {status}): {msg}"),
        });
    }
    let text = match protocol {
        Protocol::Anthropic => {
            if v.get("stop_reason").and_then(Value::as_str) == Some("refusal") {
                return Err("The AI declined to answer that.".into());
            }
            v.get("content")
                .and_then(Value::as_array)
                .map(|blocks| {
                    blocks
                        .iter()
                        .filter(|b| b.get("type").and_then(Value::as_str) == Some("text"))
                        .filter_map(|b| b.get("text").and_then(Value::as_str))
                        .collect::<Vec<_>>()
                        .join("")
                })
                .unwrap_or_default()
        }
        Protocol::OpenAiCompatible => {
            let msg = v.pointer("/choices/0/message/content");
            match msg {
                Some(Value::String(s)) => s.clone(),
                // Some providers return content as an array of parts.
                Some(Value::Array(parts)) => parts
                    .iter()
                    .filter_map(|p| p.get("text").and_then(Value::as_str))
                    .collect::<Vec<_>>()
                    .join(""),
                _ => String::new(),
            }
        }
    };
    let text = text.trim().to_string();
    if text.is_empty() {
        Err("The AI returned an empty reply.".into())
    } else {
        Ok(text)
    }
}

/// Send one request and return the reply text.
pub async fn complete(
    client: &reqwest::Client,
    endpoint: &AiEndpoint,
    system: &str,
    messages: &[Message],
) -> Result<String, String> {
    let req = build_request(endpoint, system, messages);
    let host = req
        .url
        .split("://")
        .nth(1)
        .and_then(|r| r.split('/').next())
        .unwrap_or("the AI service")
        .to_string();
    let mut rb = client.post(&req.url).json(&req.body);
    for (k, v) in &req.headers {
        rb = rb.header(*k, v);
    }
    let resp = rb.send().await.map_err(|e| {
        if e.is_timeout() {
            format!("{host} took too long to answer.")
        } else {
            format!("Couldn't reach {host}. Check your connection and Settings → AI.")
        }
    })?;
    let status = resp.status().as_u16();
    let body = resp
        .text()
        .await
        .map_err(|_| format!("The reply from {host} was cut off."))?;
    parse_response(endpoint.protocol, status, &body)
}

/// An HTTP client suited to AI requests (long timeout).
pub fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ss_core::ai::AiSettings;

    fn endpoint(provider: AiProvider, key: Option<&str>, model: &str) -> AiEndpoint {
        AiSettings {
            enabled: true,
            provider,
            base_url: if provider == AiProvider::Custom {
                "https://gw.example/v1".into()
            } else {
                String::new()
            },
            api_key: key.map(str::to_string),
            model: model.into(),
        }
        .endpoint()
        .unwrap()
    }

    #[test]
    fn anthropic_request_shape() {
        let e = endpoint(AiProvider::Anthropic, Some("sk-ant"), "");
        let r = build_request(
            &e,
            "sys",
            &[
                Message::user("hi"),
                Message::assistant("yo"),
                Message::user("rain?"),
            ],
        );
        assert_eq!(r.url, "https://api.anthropic.com/v1/messages");
        assert!(r.headers.contains(&("x-api-key", "sk-ant".into())));
        assert!(r
            .headers
            .contains(&("anthropic-version", ANTHROPIC_VERSION.into())));
        assert_eq!(r.body["model"], "claude-opus-5-5");
        assert_eq!(r.body["system"], "sys");
        assert_eq!(r.body["fallbacks"], "default");
        assert_eq!(r.body["messages"].as_array().unwrap().len(), 3);
        assert!(r.body.get("temperature").is_none());
        assert!(r.body.get("thinking").is_none());
    }

    #[test]
    fn openai_compatible_request_shape() {
        let e = endpoint(
            AiProvider::OpenRouter,
            Some("or-key"),
            "anthropic/claude-opus-5-5",
        );
        let r = build_request(&e, "sys", &[Message::user("hi")]);
        assert_eq!(r.url, "https://openrouter.ai/api/v1/chat/completions");
        assert!(r
            .headers
            .contains(&("authorization", "Bearer or-key".into())));
        assert!(r.headers.iter().any(|(k, _)| *k == "x-title"));
        let msgs = r.body["messages"].as_array().unwrap();
        assert_eq!(msgs[0]["role"], "system");
        assert_eq!(msgs[1]["content"], "hi");

        let local = endpoint(AiProvider::Ollama, None, "llama3.2");
        let r = build_request(&local, "s", &[Message::user("x")]);
        assert_eq!(r.url, "http://localhost:11434/v1/chat/completions");
        assert!(!r.headers.iter().any(|(k, _)| *k == "authorization"));

        let custom = endpoint(AiProvider::Custom, Some("k"), "m");
        assert_eq!(
            build_request(&custom, "s", &[]).url,
            "https://gw.example/v1/chat/completions"
        );
    }

    #[test]
    fn parses_both_protocols() {
        let a = r#"{"content":[{"type":"thinking","thinking":""},{"type":"text","text":"Dry "},{"type":"text","text":"today."}],"stop_reason":"end_turn"}"#;
        assert_eq!(
            parse_response(Protocol::Anthropic, 200, a).unwrap(),
            "Dry today."
        );
        let o = r#"{"choices":[{"message":{"role":"assistant","content":" Sunny. "}}]}"#;
        assert_eq!(
            parse_response(Protocol::OpenAiCompatible, 200, o).unwrap(),
            "Sunny."
        );
        let parts = r#"{"choices":[{"message":{"content":[{"type":"text","text":"A"},{"type":"text","text":"B"}]}}]}"#;
        assert_eq!(
            parse_response(Protocol::OpenAiCompatible, 200, parts).unwrap(),
            "AB"
        );
    }

    #[test]
    fn errors_are_readable() {
        let refusal = r#"{"content":[],"stop_reason":"refusal"}"#;
        assert!(parse_response(Protocol::Anthropic, 200, refusal)
            .unwrap_err()
            .contains("declined"));
        let bad = r#"{"error":{"message":"invalid key"}}"#;
        assert!(parse_response(Protocol::OpenAiCompatible, 401, bad)
            .unwrap_err()
            .contains("rejected"));
        let nf = r#"{"error":{"message":"model nope not found"}}"#;
        assert!(parse_response(Protocol::OpenAiCompatible, 404, nf)
            .unwrap_err()
            .contains("nope"));
        assert!(parse_response(Protocol::OpenAiCompatible, 502, "<html>")
            .unwrap_err()
            .contains("502"));
        assert!(parse_response(Protocol::OpenAiCompatible, 200, r#"{"choices":[]}"#).is_err());
    }
}
