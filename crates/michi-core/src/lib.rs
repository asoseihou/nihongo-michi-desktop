//! Shared logic of Nihongo Michi, used by the Tauri backend (native) and the
//! Leptos UI (wasm32). Everything here is a faithful port of the artifact's
//! JavaScript (`src/01-core.js`, `03-srs.js`, `10-today.js`) plus the new
//! agenda model (events with RRULE recurrence).

pub mod agenda;
pub mod de;
pub mod docs;
pub mod ops;
pub mod routine;
pub mod rrule;
pub mod srs;
pub mod time;
pub mod trip;

pub use docs::{DayDoc, Profile};
pub use routine::Block;
pub use srs::Card;
