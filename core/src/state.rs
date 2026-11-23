//! Office state (task D3, decisions E4 and E5 in `docs/FASE2-PLANO.md`).
//!
//! [`Office`] turns [`ReducedEvent`]s into the office's own format
//! ([`OfficeEvent`], decision E4) and keeps a picture of who is in the office
//! and what each one is doing ([`OfficeSnapshot`]), so a page that opens late
//! gets the picture first and then the changes (decision E5).
//!
//! The rules come from what the hooks expose (`docs/FASE0-HOOKS.md`):
//!
//! - An event without `agent_id` is the main agent's (the "boss").
//! - A subagent's own events carry only `agent_id` and `agent_type`; its
//!   name comes from the `Agent` call that created it. The link between that
//!   call (`tool_use_id`) and the new `agent_id` is only certain when the
//!   call's `PostToolUse` arrives (foreground) or right away for background
//!   agents and teammates. Until then, a new subagent takes the oldest pending
//!   call of the same type, and the name is corrected later if needed.
//! - A `SubagentStart` for a known `agent_id` is the agent coming back (it
//!   was sent a message).
//! - Teammates (agent teams) carry their name as `agent_type`.
//! - Events with an `agent_id` but no `agent_type` come from Claude Code's
//!   own helper agents and are ignored.
//!
//! Everything it holds is bounded: sessions, agents per session, pending
//! calls and tasks.

use std::collections::{HashMap, VecDeque};

use serde::Serialize;

use crate::events::ReducedEvent;

/// Version of [`OfficeEvent`] and [`OfficeSnapshot`] on the wire.
pub const OFFICE_EVENT_VERSION: u32 = 1;
/// Id of the main agent in every session.
pub const MAIN_AGENT: &str = "main";
/// Sessions kept at once; the one idle the longest goes first.
pub const MAX_SESSIONS: usize = 16;
/// `result` of the [`Kind::SessionEnd`] sent when a session is dropped to
/// make room for a new one ([`MAX_SESSIONS`]), and of the [`Kind::End`] sent
/// when an agent is dropped for the same reason ([`MAX_AGENTS`]).
pub const EVICTED: &str = "evicted";
/// Agents kept per session; the one idle the longest goes first.
pub const MAX_AGENTS: usize = 64;
/// `Agent` calls waiting for their subagent, per session.
const MAX_PENDING: usize = 64;
/// Tasks kept per session.
const MAX_TASKS: usize = 64;

/// What happened, in the office's terms (decision E4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// The agent is in the office (arrived, came back, or its name or color
    /// became known). Sent again whenever those change.
    Start,
    /// The agent is using a tool: `action` and `target`.
    Tool,
    /// The agent talks to another: `to` and `text`.
    Message,
    /// The agent waits: for permission, for the user, or idle.
    Wait,
    /// The agent finished: `result`.
    End,
    /// Something failed.
    Error,
    /// A task was created or completed.
    Task,
    /// The session is gone and everyone in it left: `result` is Claude
    /// Code's `reason` when it gave one, or [`EVICTED`]. Always the last
    /// event of a session, with `agent` = [`MAIN_AGENT`], after an `End` for
    /// each agent.
    #[serde(rename = "session_end")]
    SessionEnd,
}

/// One change in the office (decision E4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OfficeEvent {
    pub v: u32,
    pub session: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pane: Option<String>,
    /// [`MAIN_AGENT`] or the subagent's `agent_id`.
    pub agent: String,
    pub kind: Kind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    /// Agent id of the recipient when known, otherwise the name as written.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task: Option<String>,
}

/// What an agent is doing now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Activity {
    Working,
    Waiting,
    Idle,
}

/// One agent in the picture.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AgentView {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    pub activity: Activity,
    /// The last bubble: what it is doing, waiting for, said or reported.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bubble: Option<String>,
}

/// One task on the board.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TaskView {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    pub done: bool,
}

/// One `claude` session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SessionView {
    pub session: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pane: Option<String>,
    /// The main agent first, then the others in order of arrival.
    pub agents: Vec<AgentView>,
    pub tasks: Vec<TaskView>,
}

/// The whole office, for a page that opens late (decision E5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
pub struct OfficeSnapshot {
    pub v: u32,
    pub sessions: Vec<SessionView>,
}

/// An `Agent` call whose subagent has not been linked yet.
#[derive(Debug, Clone)]
struct Pending {
    tool_use_id: Option<String>,
    name: Option<String>,
    role: Option<String>,
}

#[derive(Debug, Clone)]
struct Agent {
    view: AgentView,
    /// The `Agent` call it was matched to, so a later certain link can
    /// correct a guess.
    tool_use_id: Option<String>,
    /// Report handed over with `SubagentHandback`, shown when it stops.
    handback: Option<String>,
    last_seen: u64,
}

#[derive(Debug, Clone)]
struct Session {
    pane: Option<String>,
    agents: Vec<Agent>,
    pending: VecDeque<Pending>,
    /// Name given in each recent `Agent` call, by `tool_use_id`, to correct
    /// a guess once the certain link arrives.
    calls: VecDeque<(String, Option<String>)>,
    /// Certain links from `PostToolUse Agent`: agent id → (name, color).
    linked: HashMap<String, (Option<String>, Option<String>)>,
    tasks: Vec<TaskView>,
    last_seen: u64,
}

impl Session {
    fn new(pane: Option<String>) -> Self {
        Self {
            pane,
            agents: Vec::new(),
            pending: VecDeque::new(),
            calls: VecDeque::new(),
            linked: HashMap::new(),
            tasks: Vec::new(),
            last_seen: 0,
        }
    }

    fn agent(&mut self, id: &str) -> Option<&mut Agent> {
        self.agents.iter_mut().find(|a| a.view.id == id)
    }

    /// Agent id for a recipient written as a name or as an id.
    fn resolve(&self, to: &str) -> String {
        let bare = to.trim_start_matches('@');
        self.agents
            .iter()
            .find(|a| a.view.id == bare || a.view.name.as_deref() == Some(bare))
            .map_or_else(|| bare.to_owned(), |a| a.view.id.clone())
    }
}

/// The office: sessions, agents and tasks, built from reduced events.
#[derive(Debug, Default)]
pub struct Office {
    sessions: HashMap<String, Session>,
    /// Arrival order of sessions, for the snapshot.
    order: Vec<String>,
    clock: u64,
}

impl Office {
    pub fn new() -> Self {
        Self::default()
    }

    /// Applies one event and returns the office events it causes (often
    /// one, sometimes none or two).
    pub fn apply(&mut self, e: &ReducedEvent) -> Vec<OfficeEvent> {
        // Claude Code's own helper agents: an id without a type.
        if e.agent_id.is_some() && e.agent_type.is_none() {
            return Vec::new();
        }
        self.clock += 1;
        let clock = self.clock;
        let mut out = Vec::new();

        if e.event == "SessionEnd" {
            if let Some(session) = self.sessions.remove(&e.session) {
                self.order.retain(|s| s != &e.session);
                out.extend(close(&e.session, &session, e.reason.as_deref()));
            }
            return out;
        }

        let session = self.session(e, clock, &mut out);
        let agent_id = e.agent_id.clone().unwrap_or_else(|| MAIN_AGENT.to_owned());

        match e.event.as_str() {
            "SubagentStart" => out.extend(arrive(session, e, &agent_id, clock)),
            "SubagentStop" => {
                out.extend(ensure(session, e, &agent_id, clock));
                let agent = session.agent(&agent_id).expect("ensured");
                let result = agent.handback.take().or_else(|| e.last_message.clone());
                agent.view.activity = Activity::Idle;
                agent.view.bubble.clone_from(&result);
                let mut ev = event(e, &session.pane, &agent_id, Kind::End);
                ev.result = result;
                out.push(ev);
            }
            "SessionStart" | "UserPromptSubmit" => {
                out.extend(ensure(session, e, MAIN_AGENT, clock));
                if e.event == "UserPromptSubmit" {
                    let action = if e.task_notification == Some(true) {
                        "report"
                    } else {
                        "task"
                    };
                    out.push(working(session, e, MAIN_AGENT, action, None));
                }
            }
            "PreToolUse" => out.extend(pre_tool(session, e, &agent_id, clock)),
            "PostToolUse" => out.extend(post_tool(session, e, clock)),
            "PostToolUseFailure" => {
                out.extend(ensure(session, e, &agent_id, clock));
                let mut ev = event(e, &session.pane, &agent_id, Kind::Error);
                ev.action.clone_from(&e.tool);
                ev.result = Some(
                    if e.interrupt == Some(true) {
                        "interrupted"
                    } else {
                        "failed"
                    }
                    .to_owned(),
                );
                out.push(ev);
            }
            "StopFailure" => {
                out.extend(ensure(session, e, MAIN_AGENT, clock));
                let mut ev = event(e, &session.pane, MAIN_AGENT, Kind::Error);
                ev.result = Some("failed".to_owned());
                out.push(ev);
            }
            "PermissionRequest" => {
                out.extend(ensure(session, e, &agent_id, clock));
                let target = detail(e);
                out.push(waiting(session, e, &agent_id, "permission", target));
            }
            "Notification" => {
                let reason = match e.notification_type.as_deref() {
                    Some("permission_prompt") => "permission",
                    Some("idle_prompt") => "user",
                    _ => return out,
                };
                out.extend(ensure(session, e, &agent_id, clock));
                out.push(waiting(session, e, &agent_id, reason, None));
            }
            "TeammateIdle" => {
                out.extend(ensure(session, e, &agent_id, clock));
                out.push(waiting(session, e, &agent_id, "idle", None));
            }
            "Stop" => {
                out.extend(ensure(session, e, MAIN_AGENT, clock));
                let agent = session.agent(MAIN_AGENT).expect("ensured");
                agent.view.activity = Activity::Idle;
                agent.view.bubble = None;
                out.push(event(e, &session.pane, MAIN_AGENT, Kind::End));
            }
            "TaskCreated" | "TaskCompleted" => {
                let done = e.event == "TaskCompleted";
                if let Some(id) = &e.task_id {
                    match session.tasks.iter_mut().find(|t| &t.id == id) {
                        Some(task) => {
                            task.done = done;
                            if e.task_subject.is_some() {
                                task.subject.clone_from(&e.task_subject);
                            }
                        }
                        None => {
                            if session.tasks.len() == MAX_TASKS {
                                session.tasks.remove(0);
                            }
                            session.tasks.push(TaskView {
                                id: id.clone(),
                                subject: e.task_subject.clone(),
                                done,
                            });
                        }
                    }
                }
                let mut ev = event(e, &session.pane, &agent_id, Kind::Task);
                ev.task.clone_from(&e.task_id);
                ev.text.clone_from(&e.task_subject);
                ev.action = Some(if done { "completed" } else { "created" }.to_owned());
                out.push(ev);
            }
            _ => {}
        }
        out
    }

    /// The whole picture, sessions in order of arrival.
    pub fn snapshot(&self) -> OfficeSnapshot {
        let sessions = self
            .order
            .iter()
            .filter_map(|id| self.sessions.get(id).map(|s| (id, s)))
            .map(|(id, s)| SessionView {
                session: id.clone(),
                pane: s.pane.clone(),
                agents: s.agents.iter().map(|a| a.view.clone()).collect(),
                tasks: s.tasks.clone(),
            })
            .collect();
        OfficeSnapshot {
            v: OFFICE_EVENT_VERSION,
            sessions,
        }
    }

    /// The session of `e`, created if needed (evicting the one idle the
    /// longest when full, whose closing events go to `out`). Its pane
    /// follows the latest event that has one.
    fn session(
        &mut self,
        e: &ReducedEvent,
        clock: u64,
        out: &mut Vec<OfficeEvent>,
    ) -> &mut Session {
        if !self.sessions.contains_key(&e.session) {
            if self.sessions.len() == MAX_SESSIONS
                && let Some(oldest) = self
                    .sessions
                    .iter()
                    .min_by_key(|(_, s)| s.last_seen)
                    .map(|(k, _)| k.clone())
                && let Some(evicted) = self.sessions.remove(&oldest)
            {
                self.order.retain(|s| s != &oldest);
                out.extend(close(&oldest, &evicted, Some(EVICTED)));
            }
            self.sessions
                .insert(e.session.clone(), Session::new(e.pane.clone()));
            self.order.push(e.session.clone());
        }
        let session = self.sessions.get_mut(&e.session).expect("inserted");
        session.last_seen = clock;
        if e.pane.is_some() {
            session.pane.clone_from(&e.pane);
        }
        session
    }
}

/// The events for a session that was removed: an `End` for each agent (the
/// last to arrive first), then the `SessionEnd`, all with `result`.
fn close(id: &str, session: &Session, result: Option<&str>) -> Vec<OfficeEvent> {
    session
        .agents
        .iter()
        .rev()
        .map(|a| (a.view.id.as_str(), Kind::End))
        .chain([(MAIN_AGENT, Kind::SessionEnd)])
        .map(|(agent, kind)| {
            let mut ev = office_event(id, &session.pane, agent, kind);
            ev.result = result.map(str::to_owned);
            ev
        })
        .collect()
}

/// A new office event for agent `agent` of the session of `e`.
fn event(e: &ReducedEvent, pane: &Option<String>, agent: &str, kind: Kind) -> OfficeEvent {
    office_event(&e.session, pane, agent, kind)
}

/// A new office event for agent `agent` of session `session`.
fn office_event(session: &str, pane: &Option<String>, agent: &str, kind: Kind) -> OfficeEvent {
    OfficeEvent {
        v: OFFICE_EVENT_VERSION,
        session: session.to_owned(),
        pane: pane.clone(),
        agent: agent.to_owned(),
        kind,
        name: None,
        role: None,
        color: None,
        action: None,
        target: None,
        to: None,
        text: None,
        result: None,
        task: None,
    }
}

/// A `Start` event with the agent's current name, role and color.
fn start_event(e: &ReducedEvent, pane: &Option<String>, agent: &AgentView) -> OfficeEvent {
    let mut ev = event(e, pane, &agent.id, Kind::Start);
    ev.name.clone_from(&agent.name);
    ev.role.clone_from(&agent.role);
    ev.color.clone_from(&agent.color);
    ev
}

/// Makes sure `id` is in the session (an agent seen for the first time
/// through one of its own events, without a `SubagentStart`).
fn ensure(session: &mut Session, e: &ReducedEvent, id: &str, clock: u64) -> Vec<OfficeEvent> {
    if let Some(agent) = session.agent(id) {
        agent.last_seen = clock;
        return Vec::new();
    }
    let role = if id == MAIN_AGENT {
        None
    } else {
        e.agent_type.clone()
    };
    let name = e
        .teammate_name
        .clone()
        .or_else(|| session.linked.get(id).and_then(|(n, _)| n.clone()));
    let color = session.linked.get(id).and_then(|(_, c)| c.clone());
    let mut out: Vec<OfficeEvent> = add_agent(
        session,
        e,
        Agent {
            view: AgentView {
                id: id.to_owned(),
                name,
                role,
                color,
                activity: Activity::Working,
                bubble: None,
            },
            tool_use_id: None,
            handback: None,
            last_seen: clock,
        },
    )
    .into_iter()
    .collect();
    let agent = session
        .agents
        .iter()
        .find(|a| a.view.id == id)
        .expect("added");
    out.push(start_event(e, &session.pane, &agent.view));
    out
}

/// Adds `agent` to the session. When the session is full, the subagent idle
/// the longest leaves first, and its `End` (`result` [`EVICTED`]) is
/// returned so the page does not keep it around.
fn add_agent(session: &mut Session, e: &ReducedEvent, agent: Agent) -> Option<OfficeEvent> {
    let mut gone = None;
    if session.agents.len() == MAX_AGENTS
        && let Some(i) = session
            .agents
            .iter()
            .enumerate()
            .filter(|(_, a)| a.view.id != MAIN_AGENT)
            .min_by_key(|(_, a)| a.last_seen)
            .map(|(i, _)| i)
    {
        let evicted = session.agents.remove(i);
        let mut ev = event(e, &session.pane, &evicted.view.id, Kind::End);
        ev.result = Some(EVICTED.to_owned());
        gone = Some(ev);
    }
    if agent.view.id == MAIN_AGENT {
        session.agents.insert(0, agent);
    } else {
        session.agents.push(agent);
    }
    gone
}

/// `SubagentStart`: a new agent arrives, or a known one comes back.
fn arrive(session: &mut Session, e: &ReducedEvent, id: &str, clock: u64) -> Vec<OfficeEvent> {
    if let Some(agent) = session.agent(id) {
        agent.last_seen = clock;
        agent.view.activity = Activity::Working;
        let view = agent.view.clone();
        return vec![start_event(e, &session.pane, &view)];
    }
    let role = e.agent_type.clone();
    let mut name = None;
    let mut color = None;
    let mut tool_use_id = None;
    if let Some((n, c)) = session.linked.get(id) {
        // Background agent or teammate: linked before it started.
        name.clone_from(n);
        color.clone_from(c);
    } else if let Some(i) = session.pending.iter().position(|p| p.role == role) {
        // Foreground: the oldest call of the same type, until PostToolUse.
        let pending = session.pending.remove(i).expect("position");
        name = pending.name;
        tool_use_id = pending.tool_use_id;
    } else {
        // A teammate whose spawn we did not see: agent_type is its name.
        name = e.teammate_name.clone();
    }
    let mut out: Vec<OfficeEvent> = add_agent(
        session,
        e,
        Agent {
            view: AgentView {
                id: id.to_owned(),
                name,
                role,
                color,
                activity: Activity::Working,
                bubble: None,
            },
            tool_use_id,
            handback: None,
            last_seen: clock,
        },
    )
    .into_iter()
    .collect();
    let view = session
        .agents
        .iter()
        .find(|a| a.view.id == id)
        .expect("added")
        .view
        .clone();
    out.push(start_event(e, &session.pane, &view));
    out
}

/// Tools that are plumbing, not work: no bubble.
fn is_silent(tool: &str) -> bool {
    matches!(tool, "ToolSearch" | "ListAgents")
}

/// The verb shown for a tool.
fn action_for(tool: &str) -> &'static str {
    match tool {
        "Read" | "NotebookRead" => "reading",
        "Write" | "Edit" | "MultiEdit" | "NotebookEdit" => "writing",
        "Bash" | "PowerShell" => "running",
        "Grep" | "Glob" => "searching",
        "WebFetch" | "WebSearch" => "browsing",
        "TaskCreate" | "TaskUpdate" | "TaskList" | "TodoWrite" => "planning",
        _ => "using",
    }
}

/// What a tool works on: the file, the command's description, the program
/// or the search pattern (all already reduced).
fn detail(e: &ReducedEvent) -> Option<String> {
    e.file
        .clone()
        .or_else(|| e.description.clone())
        .or_else(|| e.program.clone())
        .or_else(|| e.pattern.clone())
}

fn working(
    session: &mut Session,
    e: &ReducedEvent,
    id: &str,
    action: &str,
    target: Option<String>,
) -> OfficeEvent {
    let agent = session.agent(id).expect("ensured");
    agent.view.activity = Activity::Working;
    agent.view.bubble = Some(match &target {
        Some(t) => format!("{action} {t}"),
        None => action.to_owned(),
    });
    let mut ev = event(e, &session.pane, id, Kind::Tool);
    ev.action = Some(action.to_owned());
    ev.target = target;
    ev
}

fn waiting(
    session: &mut Session,
    e: &ReducedEvent,
    id: &str,
    reason: &str,
    target: Option<String>,
) -> OfficeEvent {
    let agent = session.agent(id).expect("ensured");
    agent.view.activity = Activity::Waiting;
    agent.view.bubble = Some(match &target {
        Some(t) => format!("{reason}: {t}"),
        None => reason.to_owned(),
    });
    let mut ev = event(e, &session.pane, id, Kind::Wait);
    ev.action = Some(reason.to_owned());
    ev.target = target;
    ev
}

/// `PreToolUse`: work, a message, a call to a new agent, or a handback.
fn pre_tool(session: &mut Session, e: &ReducedEvent, id: &str, clock: u64) -> Vec<OfficeEvent> {
    let Some(tool) = e.tool.as_deref() else {
        return Vec::new();
    };
    if is_silent(tool) {
        return Vec::new();
    }
    let mut out = ensure(session, e, id, clock);
    match tool {
        "Agent" | "Task" => {
            if session.pending.len() == MAX_PENDING {
                session.pending.pop_front();
            }
            session.pending.push_back(Pending {
                tool_use_id: e.tool_use_id.clone(),
                name: e.name.clone(),
                role: e.subagent_type.clone(),
            });
            if let Some(tool_use_id) = &e.tool_use_id {
                if session.calls.len() == MAX_PENDING {
                    session.calls.pop_front();
                }
                session
                    .calls
                    .push_back((tool_use_id.clone(), e.name.clone()));
            }
            let mut ev = event(e, &session.pane, id, Kind::Message);
            ev.to = e.name.clone().or_else(|| e.subagent_type.clone());
            ev.text.clone_from(&e.description);
            out.push(ev);
        }
        "SendMessage" => {
            let mut ev = event(e, &session.pane, id, Kind::Message);
            ev.to = e.to.as_deref().map(|to| session.resolve(to));
            ev.text = e.summary.clone().or_else(|| e.message_type.clone());
            if let Some(agent) = session.agent(id) {
                agent.view.bubble.clone_from(&ev.text);
            }
            out.push(ev);
        }
        "SubagentHandback" => {
            if let Some(agent) = session.agent(id) {
                agent.handback.clone_from(&e.last_message);
            }
        }
        _ => out.push(working(session, e, id, action_for(tool), detail(e))),
    }
    out
}

/// `PostToolUse`: only `Agent` matters, for the certain link to the new
/// agent (and its name and color for teammates).
fn post_tool(session: &mut Session, e: &ReducedEvent, clock: u64) -> Vec<OfficeEvent> {
    if !matches!(e.tool.as_deref(), Some("Agent" | "Task")) {
        return Vec::new();
    }
    let Some(spawned) = e.spawned_agent_id.clone() else {
        return Vec::new();
    };
    // The call this answers: no longer pending, and its name is the one.
    if let Some(i) = e.tool_use_id.as_ref().and_then(|t| {
        session
            .pending
            .iter()
            .position(|p| p.tool_use_id.as_ref() == Some(t))
    }) {
        session.pending.remove(i);
    }
    let call_name = e
        .tool_use_id
        .as_ref()
        .and_then(|t| session.calls.iter().find(|(id, _)| id == t))
        .and_then(|(_, name)| name.clone());
    let name = e.name.clone().or(call_name);
    let color = e.color.clone();

    // Another agent took this call by guess: give it back the call's place.
    if let Some(tool_use_id) = &e.tool_use_id
        && let Some(wrong) = session
            .agents
            .iter_mut()
            .find(|a| a.tool_use_id.as_ref() == Some(tool_use_id) && a.view.id != spawned)
    {
        wrong.tool_use_id = None;
    }

    match session.agent(&spawned) {
        Some(agent) => {
            agent.last_seen = clock;
            agent.tool_use_id.clone_from(&e.tool_use_id);
            let changed = (name.is_some() && agent.view.name != name)
                || (color.is_some() && agent.view.color != color);
            if name.is_some() {
                agent.view.name.clone_from(&name);
            }
            if color.is_some() {
                agent.view.color.clone_from(&color);
            }
            if changed {
                let view = agent.view.clone();
                return vec![start_event(e, &session.pane, &view)];
            }
        }
        None => {
            // Background agent or teammate: it starts after this.
            session.linked.insert(spawned, (name, color));
        }
    }
    Vec::new()
}

#[cfg(test)]
mod tests;
