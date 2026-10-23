//! Core pipeline of the Minecraft Daily Mapping Tool.
//!
//! No GUI code lives here, so the whole pipeline can be built and tested without Tauri.

pub mod anvil;
pub mod blocks;
pub mod diff;
pub mod error;
pub mod extract;
pub mod fixture;
pub mod hashing;
pub mod inflate;
pub mod model;
pub mod native;
pub mod nbt;
pub mod naming;
pub mod pipeline;
pub mod site;
pub mod store;
pub mod tiles;
pub mod topdown;

pub use error::{Error, Result};
pub use model::{Config, Dimension, SnapshotMeta};
pub use naming::{guess_stamp, Stamp, StampSource};
pub use pipeline::{delete_snapshot, import, rerender, resume, ImportRequest};
pub use store::Store;
