//! The prompts for AI summaries and answers. Requests go through `ss-ai`,
//! to whichever provider is configured in Settings → AI.

use ss_ai::Message;

/// System prompt for one-shot summaries and answers.
pub const SYSTEM: &str = "You are the forecaster inside Silver Skies, a calm desktop weather app. \
You are given a forecast for one place. Answer only from that data; never invent numbers, alerts \
or places that are not in it, and say so when the data cannot answer the question. Write in a \
warm, plain voice for a general audience. Keep replies short: two to four sentences, no headings, \
no lists, no markdown. Use the same units the data uses. For anything safety-critical, point \
people to official warnings and local authorities.";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AiTask {
    /// Summarize the forecast.
    Summarize,
    /// Answer the user's question about the forecast.
    Ask(String),
}

/// The single user message for a task, with the forecast embedded.
pub fn task_messages(task: &AiTask, digest: &str) -> Vec<Message> {
    let ask = match task {
        AiTask::Summarize => "Summarize this forecast: what it's like now, what to expect \
             through the rest of today, and anything notable in the coming days."
            .to_string(),
        AiTask::Ask(q) => format!("Answer this question about the forecast: {q}"),
    };
    vec![Message::user(format!(
        "<forecast>\n{digest}</forecast>\n\n{ask}"
    ))]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_message_embeds_forecast_and_question() {
        let m = task_messages(&AiTask::Ask("Rain later?".into()), "Now: Clear\n");
        assert_eq!(m.len(), 1);
        assert!(m[0].text.contains("<forecast>\nNow: Clear\n</forecast>"));
        assert!(m[0].text.ends_with("Rain later?"));
        let s = task_messages(&AiTask::Summarize, "x\n");
        assert!(s[0].text.contains("Summarize"));
    }
}
