//! gander-tui — the ratatui front end.
//!
//! Layout of this crate:
//!  * [`app`]        state machine: `App`, `Screen`, key handling, rendering
//!  * [`theme`]      color schemes
//!  * [`meters`]     the `Meter` trait plus drawing primitives
//!  * [`meters_impl`] concrete meters (CPU, memory, load, ...)
//!  * [`marks`]      tree-view prefixes
//!  * [`runtime`]    terminal setup, event loop, sampling thread

pub mod app;
pub mod marks;
pub mod meters;
pub mod meters_impl;
pub mod runtime;
pub mod theme;

pub use app::{App, Key, Reaction, Screen};
pub use theme::Theme;
