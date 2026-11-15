//! Optional AI: forecast summaries and plain-language answers from Claude.
//!
//! Off unless the user turns it on and supplies their own Anthropic API key
//! (Settings → AI). There is no official Anthropic SDK for Rust, so this
//! speaks the Messages API over HTTP directly. Request building and response
//! parsing are pure functions so they can be unit tested without a network.

use serde_json::{json, Value};

pub const API_URL: &str = "https://api.anthropic.com/v1/messages";
pub const API_VERSION: &str = "2026-10-21";
/// Lets the API re-run a declined request on a fallback model, routed by
/// refusal category, inside the same call.
pub const FALLBACK_BETA: &str = "server-side-fallback-2026-07-01";
pub const DEFAULT_MODEL: &str = "claude-opus-5-5";

const SYSTEM: &str = "You are the forecaster inside Silver Skies, a calm desktop weather app. \
You are given a forecast for one place. Answer only from that data; never invent \
numbers, alerts or places that are not in it, and say so when the data cannot \
answer the question. Write in a warm, plain voice for a general audience. Keep \
replies short: two to four sentences, no headings, no lists, no markdown. Use the \
same units the data uses.";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AiTask {
    /// Summarize the forecast.
    Summarize,
    /// Answer the user's question about the forecast.
    Ask(String),
}

/// JSON body for `POST /v1/messages`.
pub fn build_request(model: &str, task: &AiTask, digest: &str) -> Value {
    let ask = match task {
        AiTask::Summarize => "Summarize this forecast: what it's like now, what to expect \
             through the rest of today, and anything notable in the coming days."
            .to_string(),
        AiTask::Ask(q) => format!("Answer this question about the forecast: {q}"),
    };
    json!({
        "model": model,
        "max_tokens": 16000,
        // Short, low-stakes answers: low effort keeps them fast and cheap.
        "output_config": { "effort": "low" },
        "fallbacks": "default",
        "system": SYSTEM,
        "messages": [{
            "role": "user",
            "content": format!("<forecast>\n{digest}</forecast>\n\n{ask}"),
        }],
    })
}

/// Pull the reply text out of a Messages API response body.
pub fn parse_response(status: u16, body: &str) -> Result<String, String> {
    let v: Value =
        serde_json::from_str(body).map_err(|_| format!("AI request failed (HTTP {status})"))?;
    if !(200..300).contains(&status) {
        let msg = v
            .pointer("/error/message")
            .and_then(Value::as_str)
            .unwrap_or("unknown error");
        return Err(match status {
            401 => "The Anthropic API key was rejected. Check it in Settings → AI.".into(),
            429 => "The AI is busy right now (rate limited). Try again in a moment.".into(),
            _ => format!("AI request failed (HTTP {status}): {msg}"),
        });
    }
    if v.get("stop_reason").and_then(Value::as_str) == Some("refusal") {
        return Err("The AI declined to answer that one.".into());
    }
    let text: Vec<&str> = v
        .get("content")
        .and_then(Value::as_array)
        .map(|blocks| {
            blocks
                .iter()
                .filter(|b| b.get("type").and_then(Value::as_str) == Some("text"))
                .filter_map(|b| b.get("text").and_then(Value::as_str))
                .collect()
        })
        .unwrap_or_default();
    let joined = text.join("").trim().to_string();
    if joined.is_empty() {
        Err("The AI returned an empty reply.".into())
    } else {
        Ok(joined)
    }
}

/// Send one request. `client` should have a generous timeout (the app's has 90s).
pub async fn run(
    client: &reqwest::Client,
    api_key: &str,
    model: &str,
    task: &AiTask,
    digest: &str,
) -> Result<String, String> {
    let body = build_request(model, task, digest);
    let resp = client
        .post(API_URL)
        .header("x-api-key", api_key.trim())
        .header("anthropic-version", API_VERSION)
        .header("anthropic-beta", FALLBACK_BETA)
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Couldn't reach the AI service: {e}"))?;
    let status = resp.status().as_u16();
    let text = resp
        .text()
        .await
        .map_err(|e| format!("AI response was cut off: {e}"))?;
    parse_response(status, &text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_shape() {
        let r = build_request(
            DEFAULT_MODEL,
            &AiTask::Ask("Rain later?".into()),
            "Now: Clear\n",
        );
        assert_eq!(r["model"], "claude-opus-5-5");
        assert_eq!(r["output_config"]["effort"], "low");
        assert_eq!(r["fallbacks"], "default");
        assert!(r.get("thinking").is_none());
        assert!(r.get("temperature").is_none());
        let content = r["messages"][0]["content"].as_str().unwrap();
        assert!(content.contains("<forecast>\nNow: Clear\n</forecast>"));
        assert!(content.ends_with("Rain later?"));
        assert_eq!(r["messages"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn parses_text_blocks_and_skips_thinking() {
        let body = r#"{"content":[{"type":"thinking","thinking":""},{"type":"text","text":"Sunny "},{"type":"text","text":"all day."}],"stop_reason":"end_turn"}"#;
        assert_eq!(parse_response(200, body).unwrap(), "Sunny all day.");
    }

    #[test]
    fn refusal_and_errors() {
        let refusal = r#"{"content":[],"stop_reason":"refusal","stop_details":{"type":"refusal","category":null}}"#;
        assert!(parse_response(200, refusal)
            .unwrap_err()
            .contains("declined"));
        let bad_key = r#"{"type":"error","error":{"type":"authentication_error","message":"invalid x-api-key"}}"#;
        assert!(parse_response(401, bad_key)
            .unwrap_err()
            .contains("rejected"));
        let other = r#"{"type":"error","error":{"type":"invalid_request_error","message":"boom"}}"#;
        assert!(parse_response(400, other).unwrap_err().contains("boom"));
        assert!(parse_response(502, "<html>").unwrap_err().contains("502"));
        assert!(parse_response(200, r#"{"content":[]}"#).is_err());
    }
}
