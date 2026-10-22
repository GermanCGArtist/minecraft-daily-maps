//! Core pipeline for the Minecraft Daily Mapping Tool.
//!
//! The desktop app (`src-tauri`) is a thin shell around this crate:
//! * [`pipeline`] - import a world zip, re-render, delete
//! * [`render`]   - uNmINeD CLI call and tile normalisation
//! * [`site`]     - per-snapshot pages, day slider, `days.json`
//! * [`store`]    - the data folder layout
//! * [`model`]    - JSON formats (`config.json`, `meta.json`, `diff.json`, `days.json`)

pub mod diffcalc;
pub mod ingest;
pub mod model;
pub mod namefmt;
pub mod pipeline;
pub mod render;
pub mod site;
pub mod store;
pub mod tilepath;
pub mod timeutil;

pub use model::{Config, Dimension};
pub use pipeline::{ImportOutcome, ImportRequest};
pub use store::{SnapRef, Store};
