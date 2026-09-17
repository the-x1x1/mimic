//! Mimic native core.
//!
//! Everything in this crate is independent of the Tauri shell so it can be
//! tested headlessly in CI: the SQLite database and migrations, the canonical
//! EditDNA contract, the Lightroom loopback bridge, the Python engine sidecar
//! protocol, the persistent job system, and diagnostics.
//!
//! Non-negotiable safety rules enforced here (see CLAUDE.md):
//! * never open or mutate a Lightroom `.lrcat`;
//! * never write to source media;
//! * never drop unknown Lightroom settings during normalization;
//! * the bridge binds loopback only and requires a per-launch token;
//! * every applied edit is recorded with its before-state.

pub mod bridge;
pub mod capability;
pub mod db;
pub mod diagnostics;
pub mod edit_dna;
pub mod engine;
pub mod ids;
pub mod ingest;
pub mod jobs;
pub mod paths;
pub mod version;

pub use version::{APP_VERSION, BRIDGE_PROTOCOL_VERSION, ENGINE_PROTOCOL_VERSION};
