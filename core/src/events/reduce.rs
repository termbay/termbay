//! Reduces a raw Claude Code hook payload to a [`ReducedEvent`] (task D1,
//! decisions E2 and E10 in `docs/FASE2-PLANO.md`).
//!
//! It runs inside `termbay-hook`, before anything leaves the process, and
//! copies only the fields of a closed list. File contents, tool responses,
//! whole commands, the user's prompt, error texts and paths never get out:
//! there is no field for them.

use serde_json::Value;

use super::model::{REDUCED_EVENT_VERSION, ReducedEvent};
#[cfg(test)]
use super::text::MAX_TEXT_CHARS;
use super::text::{MAX_ID_CHARS, clean, file_name, program};

/// Reduces `raw` (the JSON a hook receives on stdin). `pane` is the
/// `TERMBAY_PANE` of the hook's environment. Returns `None` for events the
/// office does not use and for payloads without an event name or session.
pub fn reduce(raw: &Value, pane: Option<&str>) -> Option<ReducedEvent> {
    let event = text(raw, "hook_event_name")?;
    let session = text(raw, "session_id")?;
    let mut out = ReducedEvent {
        v: REDUCED_EVENT_VERSION,
        event,
        session,
        pane: pane.map(str::to_owned),
        agent_id: text(raw, "agent_id"),
        agent_type: text(raw, "agent_type"),
        tool_use_id: text(raw, "tool_use_id"),
        ..ReducedEvent::default()
    };
    if !out.is_kept() {
        return None;
    }

    match out.event.as_str() {
        "PreToolUse" | "PermissionRequest" => tool_input(raw, &mut out),
        "PostToolUse" => tool_response(raw, &mut out),
        "PostToolUseFailure" => {
            out.tool = text(raw, "tool_name");
            out.interrupt = raw.get("is_interrupt").and_then(Value::as_bool);
        }
        "SubagentStop" => {
            out.teammate_name = text(raw, "teammate_name");
            out.last_message = text(raw, "last_assistant_message");
        }
        "SubagentStart" | "TeammateIdle" => out.teammate_name = text(raw, "teammate_name"),
        "TaskCreated" | "TaskCompleted" => {
            out.task_id = text(raw, "task_id");
            out.task_subject = text(raw, "task_subject");
            out.teammate_name = text(raw, "teammate_name");
        }
        "SessionStart" => out.source = text(raw, "source"),
        "SessionEnd" => out.reason = text(raw, "reason"),
        "Notification" => out.notification_type = text(raw, "notification_type"),
        "UserPromptSubmit" => {
            let prompt = raw.get("prompt").and_then(Value::as_str).unwrap_or("");
            out.task_notification = Some(prompt.trim_start().starts_with("<task-notification>"));
        }
        "Stop" => {
            out.background_tasks = raw
                .get("background_tasks")
                .and_then(Value::as_array)
                .map(|tasks| u32::try_from(tasks.len()).unwrap_or(u32::MAX));
        }
        _ => {}
    }
    out.clamp();
    Some(out)
}

/// `PreToolUse` and `PermissionRequest`: the tool and a few safe arguments.
fn tool_input(raw: &Value, out: &mut ReducedEvent) {
    out.tool = text(raw, "tool_name");
    let Some(input) = raw.get("tool_input") else {
        return;
    };
    out.file = input
        .get("file_path")
        .and_then(Value::as_str)
        .map(file_name)
        .map(str::to_owned);
    out.description = text(input, "description");
    out.program = input
        .get("command")
        .and_then(Value::as_str)
        .and_then(program)
        .map(str::to_owned);
    out.pattern = text(input, "pattern");
    match out.tool.as_deref() {
        Some("Agent") => {
            out.name = text(input, "name");
            out.subagent_type = text(input, "subagent_type");
            out.background = input.get("run_in_background").and_then(Value::as_bool);
        }
        Some("SendMessage") => {
            out.to = text(input, "to");
            out.summary = text(input, "summary");
            out.message_type = text(input, "type");
        }
        // In auto mode a subagent hands its report over with this tool
        // instead of `last_assistant_message`.
        Some("SubagentHandback") => out.last_message = text(input, "message"),
        _ => {}
    }
}

/// `PostToolUse`: only the links between agents, never the response itself.
fn tool_response(raw: &Value, out: &mut ReducedEvent) {
    out.tool = text(raw, "tool_name");
    let Some(response) = raw.get("tool_response") else {
        return;
    };
    match out.tool.as_deref() {
        Some("Agent") => {
            out.spawned_agent_id = text(response, "agentId").or_else(|| text(response, "agent_id"));
            out.status = text(response, "status");
            out.name = text(response, "name");
            out.color = text(response, "color");
        }
        Some("SendMessage") => {
            out.success = response.get("success").and_then(Value::as_bool);
            out.resumed_agent_id = text(response, "resumedAgentId");
            if let Some(routing) = response.get("routing") {
                out.sender = text(routing, "sender");
                out.sender_color = text(routing, "senderColor");
                out.target = text(routing, "target");
                out.target_color = text(routing, "targetColor");
                out.summary = text(routing, "summary");
            }
        }
        _ => {}
    }
}

/// A string field, cleaned and cut right away so nothing long is held
/// ([`ReducedEvent::clamp`] then cuts texts to their final length).
fn text(value: &Value, key: &str) -> Option<String> {
    let s = value.get(key)?.as_str()?;
    let cleaned = clean(s, MAX_ID_CHARS);
    (!cleaned.is_empty()).then_some(cleaned)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    /// A value that must never reach the reduced event.
    const CANARY: &str = "CANARY-7f3a9c";

    fn reduced(raw: Value) -> ReducedEvent {
        reduce(&raw, Some("p3")).expect("kept event")
    }

    #[test]
    fn tool_use_keeps_the_file_name_description_and_program_only() {
        let event = reduced(json!({
            "session_id": "sess-0001", "hook_event_name": "PreToolUse", "tool_name": "Bash",
            "cwd": "/home/user/projeto", "tool_use_id": "toolu_A",
            "tool_input": {"command": "ls -la /home/user", "description": "List files in current directory"}
        }));
        assert_eq!(event.v, 1);
        assert_eq!(event.pane.as_deref(), Some("p3"));
        assert_eq!(event.tool.as_deref(), Some("Bash"));
        assert_eq!(event.program.as_deref(), Some("ls"));
        assert_eq!(
            event.description.as_deref(),
            Some("List files in current directory")
        );
        assert_eq!(event.tool_use_id.as_deref(), Some("toolu_A"));
        assert_eq!(event.agent_id, None, "main agent");

        let read = reduced(json!({
            "session_id": "s", "hook_event_name": "PreToolUse", "tool_name": "Read",
            "tool_input": {"file_path": "/home/user/projeto/src/api.ts"}
        }));
        assert_eq!(read.file.as_deref(), Some("api.ts"));
    }

    #[test]
    fn agent_call_and_its_link_to_the_agent_id() {
        let call = reduced(json!({
            "session_id": "s", "hook_event_name": "PreToolUse", "tool_name": "Agent", "tool_use_id": "toolu_B",
            "tool_input": {"description": "Ler primeira linha do README", "prompt": CANARY,
                           "subagent_type": "bruno-teste", "run_in_background": false, "name": "bruno"}
        }));
        assert_eq!(call.name.as_deref(), Some("bruno"));
        assert_eq!(call.subagent_type.as_deref(), Some("bruno-teste"));
        assert_eq!(call.background, Some(false));

        let done = reduced(json!({
            "session_id": "s", "hook_event_name": "PostToolUse", "tool_name": "Agent", "tool_use_id": "toolu_B",
            "tool_response": {"status": "completed", "agentId": "agent-bruno-01", "agentType": "bruno-teste",
                              "content": [{"type": "text", "text": CANARY}], "totalDurationMs": 2925}
        }));
        assert_eq!(done.spawned_agent_id.as_deref(), Some("agent-bruno-01"));
        assert_eq!(done.status.as_deref(), Some("completed"));

        let teammate = reduced(json!({
            "session_id": "s", "hook_event_name": "PostToolUse", "tool_name": "Agent",
            "tool_response": {"status": "teammate_spawned", "agent_id": "aana-01", "name": "ana", "color": "green",
                              "team_name": "session-0001", "tmux_pane_id": "in-process"}
        }));
        assert_eq!(teammate.spawned_agent_id.as_deref(), Some("aana-01"));
        assert_eq!(teammate.color.as_deref(), Some("green"));
    }

    #[test]
    fn messages_keep_sender_target_and_summary_but_not_the_text() {
        let sent = reduced(json!({
            "session_id": "s", "agent_id": "agent-ana-01", "agent_type": "ana-teste",
            "hook_event_name": "PreToolUse", "tool_name": "SendMessage",
            "tool_input": {"to": "bruno", "summary": "Pergunta ao Bruno sobre o README", "message": CANARY,
                           "type": "message", "content": CANARY}
        }));
        assert_eq!(sent.agent_id.as_deref(), Some("agent-ana-01"));
        assert_eq!(sent.to.as_deref(), Some("bruno"));
        assert_eq!(
            sent.summary.as_deref(),
            Some("Pergunta ao Bruno sobre o README")
        );
        assert_eq!(sent.message_type.as_deref(), Some("message"));

        let routed = reduced(json!({
            "session_id": "s", "hook_event_name": "PostToolUse", "tool_name": "SendMessage", "agent_id": "aana-01",
            "tool_response": {"success": true, "message": CANARY, "resumedAgentId": "agent-bruno-01",
                              "routing": {"sender": "ana", "senderColor": "green", "target": "@bruno",
                                          "targetColor": "blue", "summary": "Notify bruno", "content": CANARY}}
        }));
        assert_eq!(routed.success, Some(true));
        assert_eq!(routed.resumed_agent_id.as_deref(), Some("agent-bruno-01"));
        assert_eq!(routed.sender.as_deref(), Some("ana"));
        assert_eq!(routed.target_color.as_deref(), Some("blue"));
    }

    #[test]
    fn lifecycle_events() {
        let stop = reduced(json!({
            "session_id": "s", "hook_event_name": "SubagentStop", "agent_id": "a1", "agent_type": "bruno-teste",
            "agent_transcript_path": format!("/home/user/.claude/{CANARY}.jsonl"),
            "last_assistant_message": format!("# fase0 {}", "x".repeat(300))
        }));
        assert_eq!(
            stop.last_message.as_deref().map(|m| m.chars().count()),
            Some(MAX_TEXT_CHARS)
        );

        let prompt = reduced(
            json!({"session_id": "s", "hook_event_name": "UserPromptSubmit", "prompt": CANARY}),
        );
        assert_eq!(prompt.task_notification, Some(false));
        let notified = reduced(json!({
            "session_id": "s", "hook_event_name": "UserPromptSubmit", "prompt": "<task-notification>done</task-notification>"
        }));
        assert_eq!(notified.task_notification, Some(true));

        let main_stop = reduced(json!({
            "session_id": "s", "hook_event_name": "Stop", "last_assistant_message": CANARY,
            "background_tasks": [{"type": "teammate"}, {"type": "teammate"}]
        }));
        assert_eq!(main_stop.background_tasks, Some(2));
        assert_eq!(
            main_stop.last_message, None,
            "the main agent's answer is not kept"
        );

        let idle = reduced(json!({
            "session_id": "s", "hook_event_name": "TeammateIdle", "agent_id": "aana-01", "agent_type": "ana",
            "teammate_name": "ana", "team_name": "session-0001"
        }));
        assert_eq!(idle.teammate_name.as_deref(), Some("ana"));

        let task = reduced(json!({
            "session_id": "s", "hook_event_name": "TaskCreated", "task_id": "1", "task_subject": "cumprimentar",
            "task_description": CANARY
        }));
        assert_eq!(
            (task.task_id.as_deref(), task.task_subject.as_deref()),
            (Some("1"), Some("cumprimentar"))
        );

        let start = reduced(
            json!({"session_id": "s", "hook_event_name": "SessionStart", "source": "startup"}),
        );
        assert_eq!(start.source.as_deref(), Some("startup"));
        let failure = reduced(json!({
            "session_id": "s", "hook_event_name": "PostToolUseFailure", "tool_name": "Read",
            "tool_input": {"file_path": CANARY}, "error": CANARY, "is_interrupt": false
        }));
        assert_eq!(
            (failure.tool.as_deref(), failure.interrupt),
            (Some("Read"), Some(false))
        );
    }

    #[test]
    fn handback_report_is_kept_short() {
        let event = reduced(json!({
            "session_id": "s", "hook_event_name": "PreToolUse", "tool_name": "SubagentHandback", "agent_id": "a1",
            "tool_input": {"message": "Relatório: tudo certo"}
        }));
        assert_eq!(event.last_message.as_deref(), Some("Relatório: tudo certo"));
    }

    #[test]
    fn unused_and_malformed_events_are_dropped() {
        for raw in [
            json!({"session_id": "s", "hook_event_name": "MessageDisplay", "delta": CANARY}),
            json!({"session_id": "s", "hook_event_name": "PostToolBatch"}),
            json!({"session_id": "s", "hook_event_name": "WorktreeCreate"}),
            json!({"hook_event_name": "PreToolUse"}),
            json!({"session_id": "s"}),
            json!({"session_id": 3, "hook_event_name": "PreToolUse"}),
            json!([1, 2, 3]),
            json!("PreToolUse"),
        ] {
            assert_eq!(reduce(&raw, None), None, "{raw}");
        }
        // Wrong types inside a kept event are ignored, not fatal.
        let odd = reduced(json!({
            "session_id": "s", "hook_event_name": "PreToolUse", "tool_name": 5,
            "tool_input": {"file_path": ["a"], "command": {"x": 1}, "description": null}
        }));
        assert_eq!(
            (odd.tool, odd.file, odd.program, odd.description),
            (None, None, None, None)
        );
    }

    /// The canary is put in every field that carries content, commands,
    /// prompts, paths or errors, in every kept event. It must never come out.
    #[test]
    fn canary_never_reaches_the_reduced_event() {
        let secret_command =
            format!("TOKEN={CANARY} curl -H 'Authorization: {CANARY}' https://x/{CANARY}");
        for event in super::super::model::KEPT_EVENTS {
            for tool in [
                "Read",
                "Write",
                "Edit",
                "Bash",
                "Agent",
                "SendMessage",
                "Grep",
                "ListAgents",
            ] {
                let raw = json!({
                    "session_id": "s", "hook_event_name": event, "tool_name": tool,
                    "cwd": format!("/home/{CANARY}"), "transcript_path": format!("/home/{CANARY}/t.jsonl"),
                    "agent_transcript_path": format!("/home/{CANARY}/a.jsonl"),
                    "prompt": CANARY, "error": CANARY, "message": CANARY, "task_description": CANARY,
                    "tool_input": {
                        "file_path": format!("/home/{CANARY}/projeto/api.ts"), "content": CANARY,
                        "old_string": CANARY, "new_string": CANARY, "command": secret_command,
                        "prompt": CANARY, "message": CANARY, "content_blocks": [CANARY]
                    },
                    "tool_response": {
                        "content": [{"type": "text", "text": CANARY}], "file": {"content": CANARY},
                        "stdout": CANARY, "stderr": CANARY, "message": CANARY,
                        "routing": {"content": CANARY}
                    },
                    "permission_suggestions": [{"rules": [{"ruleContent": CANARY}]}]
                });
                if let Some(reduced) = reduce(&raw, Some("p1")) {
                    let wire = serde_json::to_string(&reduced).unwrap();
                    assert!(!wire.contains(CANARY), "{event}/{tool} leaked: {wire}");
                }
            }
        }
    }
}
