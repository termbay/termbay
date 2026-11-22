//! Office events between the event server and the UI (tasks B1 and D3 in
//! `docs/FASE2-PLANO.md`, decision E5).
//!
//! The server hands every accepted event to [`OfficeHub::publish`], which
//! applies it to the office state ([`Office`]) and sends the office events it
//! causes to every subscriber. The UI calls `office_subscribe` with a
//! `Channel`: it gets the picture of the office at once ([`OfficeSnapshot`]),
//! then every change.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use tauri::State;
use tauri::ipc::Channel;
use termbay_core::events::ReducedEvent;
use termbay_core::state::{Office, OfficeEvent, OfficeSnapshot};

/// Receives one office event; returns `false` once it is gone (the page
/// reloaded).
type Subscriber = Box<dyn Fn(&OfficeEvent) -> bool + Send>;

/// The office state and the live subscribers.
#[derive(Default)]
pub struct OfficeHub {
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    office: Office,
    subscribers: Vec<Subscriber>,
}

impl OfficeHub {
    /// Applies `event` to the office and sends what changed to every
    /// subscriber, forgetting those that are gone.
    pub fn publish(&self, event: ReducedEvent) {
        let mut inner = self.lock();
        for change in inner.office.apply(&event) {
            inner.subscribers.retain(|send| send(&change));
        }
    }

    /// Adds a subscriber and returns the picture of the office. Done under
    /// one lock, so no change is lost or delivered twice in between.
    pub fn subscribe(
        &self,
        send: impl Fn(&OfficeEvent) -> bool + Send + 'static,
    ) -> OfficeSnapshot {
        let mut inner = self.lock();
        inner.subscribers.push(Box::new(send));
        inner.office.snapshot()
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Subscribes the page to the office: returns its picture and sends each
/// change through `events`.
#[tauri::command]
pub fn office_subscribe(
    hub: State<'_, Arc<OfficeHub>>,
    events: Channel<OfficeEvent>,
) -> OfficeSnapshot {
    hub.subscribe(move |event| events.send(event.clone()).is_ok())
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use termbay_core::state::{Activity, Kind, MAIN_AGENT};

    use super::*;

    fn reduced(event: &str, tool: Option<&str>, file: Option<&str>) -> ReducedEvent {
        ReducedEvent {
            v: 1,
            event: event.into(),
            session: "s1".into(),
            pane: Some("2".into()),
            tool: tool.map(Into::into),
            file: file.map(Into::into),
            ..ReducedEvent::default()
        }
    }

    #[test]
    fn a_subscriber_gets_the_picture_then_the_changes() {
        let hub = OfficeHub::default();
        hub.publish(reduced("PreToolUse", Some("Read"), Some("api.ts")));

        let (tx, rx) = mpsc::channel();
        let snapshot = hub.subscribe(move |e| tx.send(e.clone()).is_ok());
        let agent = &snapshot.sessions[0].agents[0];
        assert_eq!(
            (agent.id.as_str(), agent.activity),
            (MAIN_AGENT, Activity::Working)
        );
        assert_eq!(agent.bubble.as_deref(), Some("reading api.ts"));
        assert_eq!(snapshot.sessions[0].pane.as_deref(), Some("2"));

        hub.publish(reduced("Stop", None, None));
        let change = rx.try_recv().unwrap();
        assert_eq!(
            (change.agent.as_str(), change.kind),
            (MAIN_AGENT, Kind::End)
        );
        assert!(rx.try_recv().is_err(), "earlier changes are not sent again");
    }

    #[test]
    fn every_change_of_one_event_is_sent_in_order() {
        let hub = OfficeHub::default();
        let (tx, rx) = mpsc::channel();
        hub.subscribe(move |e| tx.send(e.kind).is_ok());
        // A first event of a new session: the main agent arrives, then works.
        hub.publish(reduced("PreToolUse", Some("Read"), Some("api.ts")));
        assert_eq!(rx.try_iter().collect::<Vec<_>>(), [Kind::Start, Kind::Tool]);
    }

    #[test]
    fn a_subscriber_that_is_gone_is_forgotten() {
        let hub = OfficeHub::default();
        let (tx, rx) = mpsc::channel::<Kind>();
        hub.subscribe(move |e| tx.send(e.kind).is_ok());
        drop(rx); // the page went away
        hub.publish(reduced("SessionStart", None, None));
        assert_eq!(hub.lock().subscribers.len(), 0);
    }
}
