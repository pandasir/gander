//! gander-tui — the ratatui front end.
//!
//! Layout of this crate:
//!  * [`app`]        state machine: `App`, `Screen`, key handling, rendering
//!  * [`theme`]      color schemes
//!  * [`meters`]     the `Meter` trait plus drawing primitives
//!  * [`meters_impl`] concrete meters (CPU, memory, load, ...)
//!  * [`marks`]      tree-view prefixes
//!  * [`panels`]     the `a` affinity and `i` I/O priority panels
//!  * [`layout`]     the `panels` screen layout: system column, table, inspector
//!  * [`runtime`]    terminal setup, event loop, sampling thread

pub mod app;
pub mod marks;
pub mod meters;
pub mod meters_impl;
pub mod layout;
pub mod panels;
pub mod runtime;
pub mod setup;
pub mod theme;

pub use app::{App, Key, Reaction, Screen};
pub use theme::Theme;
