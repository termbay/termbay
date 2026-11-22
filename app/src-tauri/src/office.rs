//! Office events between the event server and the UI (task B1 in
//! `docs/FASE2-PLANO.md`).
//!
//! The server hands every accepted event to [`OfficeHub::publish`]. The UI
//! calls `office_subscribe` with a `Channel`: it gets the recent events at
//! once, then every new one. Until the office has its own state (task D3,
//! decision E5) the events go as they are: already reduced, never raw.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use tauri::State;
use tauri::ipc::Channel;
use termbay_core::events::ReducedEvent;

/// How many past events a new subscriber gets.
pub const RECENT_EVENTS: usize = 100;

/// Receives one event; returns `false` once it is gone (the page reloaded).
type Subscriber = Box<dyn Fn(&ReducedEvent) -> bool + Send>;

/// Recent events and the live subscribers.
#[derive(Default)]
pub struct OfficeHub {
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    recent: VecDeque<ReducedEvent>,
    subscribers: Vec<Subscriber>,
}

impl OfficeHub {
    /// Keeps `event` among the recent ones and sends it to every subscriber,
    /// forgetting those that are gone.
    pub fn publish(&self, event: ReducedEvent) {
        let mut inner = self.lock();
        inner.subscribers.retain(|send| send(&event));
        if inner.recent.len() == RECENT_EVENTS {
            inner.recent.pop_front();
        }
        inner.recent.push_back(event);
    }

    /// Adds a subscriber and returns the recent events, oldest first. Done
    /// under one lock, so no event is lost or delivered twice in between.
    pub fn subscribe(
        &self,
        send: impl Fn(&ReducedEvent) -> bool + Send + 'static,
    ) -> Vec<ReducedEvent> {
        let mut inner = self.lock();
        inner.subscribers.push(Box::new(send));
        inner.recent.iter().cloned().collect()
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Subscribes the page to office events: returns the recent ones and sends
/// each new one through `events`.
#[tauri::command]
pub fn office_subscribe(
    hub: State<'_, Arc<OfficeHub>>,
    events: Channel<ReducedEvent>,
) -> Vec<ReducedEvent> {
    hub.subscribe(move |event| events.send(event.clone()).is_ok())
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use super::*;

    fn event(n: usize) -> ReducedEvent {
        ReducedEvent {
            v: 1,
            event: "PreToolUse".into(),
            session: format!("s{n}"),
            ..ReducedEvent::default()
        }
    }

    #[test]
    fn a_subscriber_gets_the_recent_events_then_the_new_ones() {
        let hub = OfficeHub::default();
        hub.publish(event(1));
        hub.publish(event(2));

        let (tx, rx) = mpsc::channel();
        let recent = hub.subscribe(move |e| tx.send(e.session.clone()).is_ok());
        assert_eq!(
            recent
                .iter()
                .map(|e| e.session.as_str())
                .collect::<Vec<_>>(),
            ["s1", "s2"]
        );

        hub.publish(event(3));
        assert_eq!(rx.try_recv().unwrap(), "s3");
        assert!(rx.try_recv().is_err(), "old events are not sent again");
    }

    #[test]
    fn keeps_only_the_last_events() {
        let hub = OfficeHub::default();
        for n in 0..RECENT_EVENTS + 5 {
            hub.publish(event(n));
        }
        let recent = hub.subscribe(|_| true);
        assert_eq!(recent.len(), RECENT_EVENTS);
        assert_eq!(recent[0].session, "s5");
        assert_eq!(
            recent.last().unwrap().session,
            format!("s{}", RECENT_EVENTS + 4)
        );
    }

    #[test]
    fn a_subscriber_that_is_gone_is_forgotten() {
        let hub = OfficeHub::default();
        let (tx, rx) = mpsc::channel::<String>();
        hub.subscribe(move |e| tx.send(e.session.clone()).is_ok());
        drop(rx); // the page went away
        hub.publish(event(1));
        assert_eq!(hub.lock().subscribers.len(), 0);
    }
}
