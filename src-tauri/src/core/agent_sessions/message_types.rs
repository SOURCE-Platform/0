use serde::Serialize;

/// Who a message in a conversation came from, as the hub shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum MessageRole {
    /// Typed or spoken by you.
    User,
    /// The agent's reply text.
    Assistant,
    /// A message another agent session sent in, never shown as if you wrote it.
    Peer,
    /// The agent used a tool; `text` is the tool's name.
    Tool,
}

/// One readable entry in a conversation. Thinking and raw tool output are left
/// out: the hub shows what was said and what was done, not internals.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentMessage {
    pub id: String,
    pub role: MessageRole,
    pub text: String,
    pub at_ms: i64,
}
