//! The reduced hook event: what `termbay-hook` sends and the event server
//! accepts (section 3.2 of `docs/FASE2-PLANO.md`).
//!
//! It is a closed list of optional fields. Anything else in a payload is
//! dropped when it is reduced, and again when the server deserializes it,
//! because unknown fields have nowhere to go.

use serde::{Deserialize, Serialize};

use super::text::{MAX_ID_CHARS, MAX_TEXT_CHARS, clean};

/// Version of [`ReducedEvent`] on the wire.
pub const REDUCED_EVENT_VERSION: u32 = 1;

/// Hook events that are kept. Every other event is dropped by the reducer
/// and refused by the server.
pub const KEPT_EVENTS: &[&str] = &[
    "SessionStart",
    "SessionEnd",
    "UserPromptSubmit",
    "PreToolUse",
    "PostToolUse",
    "PostToolUseFailure",
    "PermissionRequest",
    "Notification",
    "SubagentStart",
    "SubagentStop",
    "Stop",
    "StopFailure",
    "TaskCreated",
    "TaskCompleted",
    "TeammateIdle",
];

/// One hook event, reduced to what the office needs.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ReducedEvent {
    /// Wire version, [`REDUCED_EVENT_VERSION`].
    pub v: u32,
    /// Claude Code's `hook_event_name`, one of [`KEPT_EVENTS`].
    pub event: String,
    /// Claude Code's `session_id`.
    pub session: String,

    // Ids. Absent `agent_id` means the main agent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pane: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_use_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,

    // Tool use (PreToolUse, PermissionRequest, PostToolUse, PostToolUseFailure).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    /// File name of `tool_input.file_path`, never the path.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// First word of `tool_input.command`, never the whole command.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub program: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pattern: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interrupt: Option<bool>,

    // Agent tool: who is being called, and the link to its agent id.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subagent_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub background: Option<bool>,
    /// `tool_response.agentId` (or `agent_id` for a spawned teammate).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spawned_agent_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,

    // SendMessage.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub success: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resumed_agent_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sender: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sender_color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_color: Option<String>,

    // Agents and teammates.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub teammate_name: Option<String>,
    /// First characters of `last_assistant_message` (SubagentStop) or of a
    /// `SubagentHandback` report.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_message: Option<String>,
    /// How many background tasks were still running at `Stop`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub background_tasks: Option<u32>,

    // Tasks.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_subject: Option<String>,

    // Session and turn.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notification_type: Option<String>,
    /// The prompt was a `<task-notification>` (a background agent finished).
    /// The prompt text itself is never kept.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_notification: Option<bool>,
}

impl ReducedEvent {
    /// Whether the event is one the office uses.
    pub fn is_kept(&self) -> bool {
        KEPT_EVENTS.contains(&self.event.as_str())
    }

    /// Cleans every text field in place: ids to [`MAX_ID_CHARS`], everything
    /// else to [`MAX_TEXT_CHARS`], control characters removed, empty strings
    /// turned into `None`. The reducer calls it, and the server calls it
    /// again on whatever a local client sends.
    pub fn clamp(&mut self) {
        let id = |s: &mut String| *s = clean(s, MAX_ID_CHARS);
        id(&mut self.event);
        id(&mut self.session);
        for field in [
            &mut self.pane,
            &mut self.agent_id,
            &mut self.agent_type,
            &mut self.tool_use_id,
            &mut self.task_id,
            &mut self.spawned_agent_id,
            &mut self.resumed_agent_id,
        ] {
            clamp_option(field, MAX_ID_CHARS);
        }
        for field in [
            &mut self.tool,
            &mut self.file,
            &mut self.description,
            &mut self.program,
            &mut self.pattern,
            &mut self.name,
            &mut self.subagent_type,
            &mut self.status,
            &mut self.color,
            &mut self.to,
            &mut self.summary,
            &mut self.message_type,
            &mut self.sender,
            &mut self.sender_color,
            &mut self.target,
            &mut self.target_color,
            &mut self.teammate_name,
            &mut self.last_message,
            &mut self.task_subject,
            &mut self.source,
            &mut self.reason,
            &mut self.notification_type,
        ] {
            clamp_option(field, MAX_TEXT_CHARS);
        }
    }
}

fn clamp_option(field: &mut Option<String>, max: usize) {
    if let Some(text) = field.take() {
        let cleaned = clean(&text, max);
        if !cleaned.is_empty() {
            *field = Some(cleaned);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_fields_are_dropped_on_deserialize() {
        let event: ReducedEvent = serde_json::from_str(
            r#"{"v":1,"event":"PreToolUse","session":"s1","tool":"Read","content":"SECRET","command":"SECRET"}"#,
        )
        .unwrap();
        assert_eq!(event.tool.as_deref(), Some("Read"));
        let back = serde_json::to_string(&event).unwrap();
        assert!(!back.contains("SECRET"), "{back}");
    }

    #[test]
    fn clamp_cuts_texts_and_ids_and_drops_empty_strings() {
        let mut event = ReducedEvent {
            event: "PreToolUse".into(),
            session: "s".repeat(500),
            description: Some(format!("\u{1b}]0;title\u{7}{}", "d".repeat(500))),
            summary: Some(" \n ".into()),
            ..ReducedEvent::default()
        };
        event.clamp();
        assert_eq!(event.session.chars().count(), MAX_ID_CHARS);
        let description = event.description.unwrap();
        assert_eq!(description.chars().count(), MAX_TEXT_CHARS);
        assert!(!description.contains('\u{1b}'));
        assert_eq!(event.summary, None);
    }

    #[test]
    fn only_listed_events_are_kept() {
        let mut event = ReducedEvent {
            event: "PreToolUse".into(),
            ..ReducedEvent::default()
        };
        assert!(event.is_kept());
        event.event = "MessageDisplay".into();
        assert!(!event.is_kept());
    }
}
