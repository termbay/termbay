//! Office events (phase 2, `docs/FASE2-PLANO.md`).
//!
//! - [`reduce`]: raw Claude Code hook payload → [`ReducedEvent`] (task D1).
//!   Runs in `termbay-hook`, so nothing beyond the closed list of fields
//!   leaves the hook process.
//! - [`server`]: the local server that receives reduced events (task A1).
//! - [`model`]: the reduced event shared by both.
//! - [`text`]: cleaning and cutting of everything a bubble may show.
//! - [`env`]: the variables that tell `termbay-hook` where the app is.

pub mod env;
pub mod model;
pub mod reduce;
pub mod server;
pub mod text;

pub use env::HookEnv;
pub use model::{KEPT_EVENTS, REDUCED_EVENT_VERSION, ReducedEvent};
pub use reduce::reduce;
pub use server::EventServer;
