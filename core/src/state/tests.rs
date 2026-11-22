//! Sequences built from the order of events documented in
//! `docs/FASE0-HOOKS.md` (sessions A to G), with the fields the reducer
//! keeps. The raw recordings stayed out of the repository on purpose.

use serde_json::{Value, json};

use super::*;

/// A reduced event from a JSON object (fields as `ReducedEvent` names them).
fn ev(fields: Value) -> ReducedEvent {
    let mut value = json!({ "v": 1, "session": "s1", "pane": "3" });
    for (k, v) in fields.as_object().unwrap() {
        value[k] = v.clone();
    }
    serde_json::from_value(value).unwrap()
}

/// Applies every event and returns all office events.
fn run(office: &mut Office, events: &[Value]) -> Vec<OfficeEvent> {
    events
        .iter()
        .flat_map(|e| office.apply(&ev(e.clone())))
        .collect()
}

fn agents(office: &Office) -> Vec<(String, Option<String>, Activity)> {
    office.snapshot().sessions[0]
        .agents
        .iter()
        .map(|a| (a.id.clone(), a.name.clone(), a.activity))
        .collect()
}

fn kinds(events: &[OfficeEvent]) -> Vec<(String, Kind)> {
    events.iter().map(|e| (e.agent.clone(), e.kind)).collect()
}

#[test]
fn main_agent_works_and_finishes_a_turn() {
    let mut office = Office::new();
    let out = run(
        &mut office,
        &[
            json!({"event": "SessionStart", "source": "startup"}),
            json!({"event": "UserPromptSubmit", "task_notification": false}),
            json!({"event": "PreToolUse", "tool": "Read", "file": "api.ts", "tool_use_id": "t1"}),
            json!({"event": "PreToolUse", "tool": "Bash", "program": "npm", "description": "Run the tests"}),
            json!({"event": "PreToolUse", "tool": "ToolSearch"}),
            json!({"event": "Stop"}),
        ],
    );
    assert_eq!(
        kinds(&out),
        [
            ("main".into(), Kind::Start),
            ("main".into(), Kind::Tool),
            ("main".into(), Kind::Tool),
            ("main".into(), Kind::Tool),
            ("main".into(), Kind::End),
        ]
    );
    assert_eq!(
        (out[1].action.as_deref(), out[1].target.as_deref()),
        (Some("task"), None)
    );
    assert_eq!(
        (out[2].action.as_deref(), out[2].target.as_deref()),
        (Some("reading"), Some("api.ts"))
    );
    assert_eq!(
        out[3].target.as_deref(),
        Some("Run the tests"),
        "description first, then the program"
    );
    assert!(
        out.iter()
            .all(|e| e.pane.as_deref() == Some("3") && e.session == "s1")
    );
    assert_eq!(agents(&office), [("main".into(), None, Activity::Idle)]);
}

/// Session A: a foreground subagent. The link to its call is certain only
/// at the end, but with one call of its type the guess is right at once.
#[test]
fn foreground_subagent_gets_its_name_from_the_call() {
    let mut office = Office::new();
    let out = run(
        &mut office,
        &[
            json!({"event": "PreToolUse", "tool": "Agent", "tool_use_id": "tB", "name": "bruno",
               "subagent_type": "bruno-teste", "description": "Ler primeira linha do README"}),
            json!({"event": "SubagentStart", "agent_id": "a-bruno", "agent_type": "bruno-teste"}),
            json!({"event": "PreToolUse", "agent_id": "a-bruno", "agent_type": "bruno-teste", "tool": "Read", "file": "README.md"}),
            json!({"event": "SubagentStop", "agent_id": "a-bruno", "agent_type": "bruno-teste", "last_message": "# fase0"}),
            json!({"event": "PostToolUse", "tool": "Agent", "tool_use_id": "tB", "spawned_agent_id": "a-bruno", "status": "completed"}),
        ],
    );
    assert_eq!(
        kinds(&out),
        [
            ("main".into(), Kind::Start),
            ("main".into(), Kind::Message),
            ("a-bruno".into(), Kind::Start),
            ("a-bruno".into(), Kind::Tool),
            ("a-bruno".into(), Kind::End),
        ]
    );
    assert_eq!(
        (out[1].to.as_deref(), out[1].text.as_deref()),
        (Some("bruno"), Some("Ler primeira linha do README"))
    );
    assert_eq!(
        (out[2].name.as_deref(), out[2].role.as_deref()),
        (Some("bruno"), Some("bruno-teste"))
    );
    assert_eq!(out[4].result.as_deref(), Some("# fase0"));
    // The certain link agreed with the guess: no correction sent.
    assert_eq!(
        agents(&office)[1],
        ("a-bruno".into(), Some("bruno".into()), Activity::Idle)
    );
}

/// Session B: two subagents of different types in parallel, started in the
/// opposite order of their calls. Matching by type gets both right.
#[test]
fn parallel_subagents_of_different_types() {
    let mut office = Office::new();
    run(
        &mut office,
        &[
            json!({"event": "PreToolUse", "tool": "Agent", "tool_use_id": "t1", "name": "ana", "subagent_type": "ana-teste"}),
            json!({"event": "PreToolUse", "tool": "Agent", "tool_use_id": "t2", "name": "bruno", "subagent_type": "bruno-teste"}),
            json!({"event": "SubagentStart", "agent_id": "a2", "agent_type": "bruno-teste"}),
            json!({"event": "SubagentStart", "agent_id": "a1", "agent_type": "ana-teste"}),
        ],
    );
    let names: Vec<_> = agents(&office)
        .into_iter()
        .map(|(id, name, _)| (id, name))
        .collect();
    assert_eq!(
        names,
        [
            ("main".into(), None),
            ("a2".into(), Some("bruno".into())),
            ("a1".into(), Some("ana".into())),
        ]
    );
}

/// Two calls of the same type, started in the opposite order: the guess is
/// wrong at first and the certain links fix both names.
#[test]
fn a_wrong_guess_is_corrected_by_the_certain_link() {
    let mut office = Office::new();
    run(
        &mut office,
        &[
            json!({"event": "PreToolUse", "tool": "Agent", "tool_use_id": "tx", "name": "x", "subagent_type": "general-purpose"}),
            json!({"event": "PreToolUse", "tool": "Agent", "tool_use_id": "ty", "name": "y", "subagent_type": "general-purpose"}),
            // The agent of call `ty` starts first and takes `x` by guess.
            json!({"event": "SubagentStart", "agent_id": "a-y", "agent_type": "general-purpose"}),
            json!({"event": "SubagentStart", "agent_id": "a-x", "agent_type": "general-purpose"}),
        ],
    );
    assert_eq!(agents(&office)[1].1.as_deref(), Some("x"), "the guess");

    let fix = run(
        &mut office,
        &[
            json!({"event": "PostToolUse", "tool": "Agent", "tool_use_id": "ty", "spawned_agent_id": "a-y", "status": "completed"}),
            json!({"event": "PostToolUse", "tool": "Agent", "tool_use_id": "tx", "spawned_agent_id": "a-x", "status": "completed"}),
        ],
    );
    assert_eq!(
        kinds(&fix),
        [("a-y".into(), Kind::Start), ("a-x".into(), Kind::Start)]
    );
    assert_eq!(fix[0].name.as_deref(), Some("y"));
    assert_eq!(fix[1].name.as_deref(), Some("x"));
    let names: Vec<_> = agents(&office)
        .into_iter()
        .map(|(id, name, _)| (id, name))
        .collect();
    assert_eq!(
        names[1..],
        [
            ("a-y".into(), Some("y".into())),
            ("a-x".into(), Some("x".into()))
        ]
    );
}

/// Session C: Ana messages Bruno, who had finished; Bruno comes back.
#[test]
fn a_message_brings_a_finished_agent_back() {
    let mut office = Office::new();
    run(
        &mut office,
        &[
            json!({"event": "PreToolUse", "tool": "Agent", "tool_use_id": "tb", "name": "bruno", "subagent_type": "bruno-teste"}),
            json!({"event": "SubagentStart", "agent_id": "a-bruno", "agent_type": "bruno-teste"}),
            json!({"event": "SubagentStop", "agent_id": "a-bruno", "agent_type": "bruno-teste", "last_message": "pronto"}),
            json!({"event": "PreToolUse", "tool": "Agent", "tool_use_id": "ta", "name": "ana", "subagent_type": "ana-teste"}),
            json!({"event": "SubagentStart", "agent_id": "a-ana", "agent_type": "ana-teste"}),
        ],
    );
    let out = run(
        &mut office,
        &[
            json!({"event": "PreToolUse", "agent_id": "a-ana", "agent_type": "ana-teste", "tool": "SendMessage",
               "to": "bruno", "summary": "Pergunta sobre o README", "message_type": "message"}),
            json!({"event": "PostToolUse", "agent_id": "a-ana", "agent_type": "ana-teste", "tool": "SendMessage",
               "success": true, "resumed_agent_id": "a-bruno"}),
            json!({"event": "SubagentStart", "agent_id": "a-bruno", "agent_type": "bruno-teste"}),
        ],
    );
    assert_eq!(
        kinds(&out),
        [
            ("a-ana".into(), Kind::Message),
            ("a-bruno".into(), Kind::Start)
        ]
    );
    assert_eq!(
        out[0].to.as_deref(),
        Some("a-bruno"),
        "the name resolves to the agent id"
    );
    assert_eq!(out[0].text.as_deref(), Some("Pergunta sobre o README"));
    assert_eq!(out[1].name.as_deref(), Some("bruno"));
    assert_eq!(agents(&office).len(), 3, "Bruno came back, not a new agent");
    assert_eq!(agents(&office)[1].2, Activity::Working);
}

/// Session E: a background agent is linked before it starts.
#[test]
fn background_agent_is_linked_before_it_starts() {
    let mut office = Office::new();
    let out = run(
        &mut office,
        &[
            json!({"event": "PreToolUse", "tool": "Agent", "tool_use_id": "tbg", "subagent_type": "general-purpose",
               "description": "Count entries", "background": true}),
            json!({"event": "PostToolUse", "tool": "Agent", "tool_use_id": "tbg", "spawned_agent_id": "a-bg", "status": "async_launched"}),
            json!({"event": "SubagentStart", "agent_id": "a-bg", "agent_type": "general-purpose"}),
            json!({"event": "UserPromptSubmit", "task_notification": true}),
        ],
    );
    assert_eq!(
        out.iter()
            .filter(|e| e.agent == "a-bg" && e.kind == Kind::Start)
            .count(),
        1
    );
    let last = out.last().unwrap();
    assert_eq!(
        (last.agent.as_str(), last.action.as_deref()),
        ("main", Some("report"))
    );
    // The call is no longer pending: a later agent of that type does not take it.
    run(
        &mut office,
        &[
            json!({"event": "SubagentStart", "agent_id": "a-other", "agent_type": "general-purpose"}),
        ],
    );
    assert_eq!(agents(&office)[2].1, None);
}

/// Session G: teammates get name and color when spawned, and their own
/// events carry the name as agent_type.
#[test]
fn teammates_with_name_and_color() {
    let mut office = Office::new();
    let out = run(
        &mut office,
        &[
            json!({"event": "PreToolUse", "tool": "Agent", "tool_use_id": "tana", "name": "ana", "subagent_type": "ana-teste"}),
            json!({"event": "PostToolUse", "tool": "Agent", "tool_use_id": "tana", "spawned_agent_id": "aana-01",
               "status": "teammate_spawned", "name": "ana", "color": "green"}),
            json!({"event": "SubagentStart", "agent_id": "aana-01", "agent_type": "ana"}),
            json!({"event": "PreToolUse", "agent_id": "aana-01", "agent_type": "ana", "tool": "SendMessage",
               "to": "bruno", "summary": "Notify bruno"}),
            json!({"event": "SubagentStop", "agent_id": "aana-01", "agent_type": "ana"}),
            json!({"event": "TeammateIdle", "agent_id": "aana-01", "agent_type": "ana", "teammate_name": "ana"}),
        ],
    );
    let start = out
        .iter()
        .find(|e| e.agent == "aana-01" && e.kind == Kind::Start)
        .unwrap();
    assert_eq!(
        (start.name.as_deref(), start.color.as_deref()),
        (Some("ana"), Some("green"))
    );
    let message = out
        .iter()
        .find(|e| e.kind == Kind::Message && e.agent == "aana-01")
        .unwrap();
    assert_eq!(
        message.to.as_deref(),
        Some("bruno"),
        "unknown recipient stays as written"
    );
    assert_eq!(out.last().unwrap().kind, Kind::Wait);
    assert_eq!(out.last().unwrap().action.as_deref(), Some("idle"));
    assert_eq!(agents(&office)[1].2, Activity::Waiting);
}

#[test]
fn helper_agents_without_a_type_are_ignored() {
    let mut office = Office::new();
    let out = run(
        &mut office,
        &[
            json!({"event": "SubagentStop", "agent_id": "rand-1", "last_message": "suggestion"}),
            json!({"event": "PreToolUse", "agent_id": "rand-1", "tool": "Read", "file": "x"}),
        ],
    );
    assert!(out.is_empty());
    assert!(office.snapshot().sessions.is_empty());
}

#[test]
fn handback_report_is_the_result_when_the_agent_stops() {
    let mut office = Office::new();
    let out = run(
        &mut office,
        &[
            json!({"event": "SubagentStart", "agent_id": "a1", "agent_type": "qa"}),
            json!({"event": "PreToolUse", "agent_id": "a1", "agent_type": "qa", "tool": "SubagentHandback", "last_message": "Relatório: ok"}),
            json!({"event": "SubagentStop", "agent_id": "a1", "agent_type": "qa", "last_message": "something else"}),
        ],
    );
    assert_eq!(out.last().unwrap().result.as_deref(), Some("Relatório: ok"));
}

#[test]
fn waits_errors_and_tasks() {
    let mut office = Office::new();
    let out = run(
        &mut office,
        &[
            json!({"event": "PermissionRequest", "tool": "Bash", "program": "curl", "description": "Fetch the page"}),
            json!({"event": "Notification", "notification_type": "idle_prompt"}),
            json!({"event": "Notification", "notification_type": "auth_success"}),
            json!({"event": "PostToolUseFailure", "tool": "Read", "interrupt": false}),
            json!({"event": "StopFailure"}),
            json!({"event": "TaskCreated", "task_id": "1", "task_subject": "cumprimentar"}),
            json!({"event": "TaskCompleted", "task_id": "1"}),
        ],
    );
    let summary: Vec<_> = out
        .iter()
        .map(|e| (e.kind, e.action.clone(), e.target.clone()))
        .collect();
    assert_eq!(
        summary,
        [
            (Kind::Start, None, None),
            (
                Kind::Wait,
                Some("permission".into()),
                Some("Fetch the page".into())
            ),
            (Kind::Wait, Some("user".into()), None),
            (Kind::Error, Some("Read".into()), None),
            (Kind::Error, None, None),
            (Kind::Task, Some("created".into()), None),
            (Kind::Task, Some("completed".into()), None),
        ]
    );
    assert_eq!(out[3].result.as_deref(), Some("failed"));
    let tasks = &office.snapshot().sessions[0].tasks;
    assert_eq!(
        tasks,
        &[TaskView {
            id: "1".into(),
            subject: Some("cumprimentar".into()),
            done: true
        }]
    );
}

#[test]
fn session_end_sends_everyone_home() {
    let mut office = Office::new();
    run(
        &mut office,
        &[
            json!({"event": "SessionStart"}),
            json!({"event": "SubagentStart", "agent_id": "a1", "agent_type": "qa"}),
        ],
    );
    let out = office.apply(&ev(json!({"event": "SessionEnd", "reason": "exit"})));
    assert_eq!(
        kinds(&out),
        [("a1".into(), Kind::End), ("main".into(), Kind::End)]
    );
    assert!(out.iter().all(|e| e.result.as_deref() == Some("exit")));
    assert!(office.snapshot().sessions.is_empty());
    assert!(office.apply(&ev(json!({"event": "SessionEnd"}))).is_empty());
}

#[test]
fn sessions_are_kept_apart_and_bounded() {
    let mut office = Office::new();
    for n in 0..MAX_SESSIONS + 3 {
        office.apply(&ev(
            json!({"event": "SessionStart", "session": format!("s{n}"), "pane": format!("{n}")}),
        ));
    }
    let snapshot = office.snapshot();
    assert_eq!(snapshot.sessions.len(), MAX_SESSIONS);
    assert_eq!(
        snapshot.sessions[0].session, "s3",
        "the oldest ones went first"
    );
    assert_eq!(
        snapshot.sessions.last().unwrap().pane.as_deref(),
        Some(&*format!("{}", MAX_SESSIONS + 2))
    );

    let mut office = Office::new();
    for n in 0..MAX_AGENTS + 5 {
        office.apply(&ev(
            json!({"event": "SubagentStart", "agent_id": format!("a{n}"), "agent_type": "qa"}),
        ));
    }
    office.apply(&ev(json!({"event": "SessionStart"})));
    let agents = &office.snapshot().sessions[0].agents;
    assert_eq!(agents.len(), MAX_AGENTS);
    assert_eq!(agents[0].id, MAIN_AGENT, "the main agent is always first");
}

#[test]
fn snapshot_and_events_serialize_in_the_e4_format() {
    let mut office = Office::new();
    let out = run(
        &mut office,
        &[json!({"event": "PreToolUse", "tool": "Read", "file": "api.ts"})],
    );
    let wire = serde_json::to_value(&out[1]).unwrap();
    assert_eq!(
        wire,
        json!({"v": 1, "session": "s1", "pane": "3", "agent": "main", "kind": "tool", "action": "reading", "target": "api.ts"})
    );
    let snapshot = serde_json::to_value(office.snapshot()).unwrap();
    assert_eq!(
        snapshot["sessions"][0]["agents"][0],
        json!({"id": "main", "activity": "working", "bubble": "reading api.ts"})
    );
}
