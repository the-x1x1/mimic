//! Mimic native core.
//!
//! Mimic learns how a person communicates and helps them draft responses that
//! sound like themselves. Everything in this crate is independent of the Tauri
//! shell so it can be tested headlessly in CI: the SQLite database and
//! migrations, the communication source connectors, the import pipeline, the
//! layered voice engine, retrieval, generation, the model-provider
//! abstraction, the persistent job system and diagnostics.
//!
//! Non-negotiable rules enforced here (see CLAUDE.md):
//! * message content never leaves the machine except through a model provider
//!   the user configured, and never appears in logs by default;
//! * nothing is treated as evidence of how the user writes unless its
//!   direction is `self`;
//! * deleting a person really deletes their material and rebuilds whatever was
//!   derived from it;
//! * no metric is shown that was not measured — "not enough data yet" is a
//!   valid answer and a fabricated percentage is not;
//! * credentials live in the OS secure store, never in the database.

pub mod assist;
pub mod dashboard;
pub mod db;
pub mod diagnostics;
pub mod encoder;
pub mod engine;
pub mod evaluation;
pub mod generation;
pub mod ids;
pub mod import;
pub mod instance;
pub mod jobs;
pub mod learning;
pub mod localmodel;
pub mod paths;
pub mod privacy;
pub mod providers;
pub mod retrieval;
pub mod situations;
pub mod sources;
pub mod version;
pub mod voice;

pub use version::{APP_VERSION, ENGINE_PROTOCOL_VERSION};
