//! Reading messages for meaning: the sentence encoder the app offers,
//! downloading it when the user asks, and how many of their messages it has
//! read.

use serde_json::{json, Value};
use tauri::State;

use crate::error::{CommandError, CommandResult};
use crate::SharedState;

/// The encoder this build offers.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OfferedEncoder {
    pub id: String,
    pub name: String,
    pub description: String,
    pub bytes: u64,
    pub license: String,
    pub homepage: String,
}

/// What is true about reading messages for meaning right now.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EncoderView {
    /// None when this build offers no encoder.
    pub offered: Option<OfferedEncoder>,
    /// Every file is downloaded, at the size its manifest pins.
    pub downloaded: bool,
    /// The engine is running the encoder — downloaded, and every file
    /// matching its SHA-256.
    pub in_use: bool,
    /// The engine's own word on which it uses and why; none while it is not
    /// running.
    pub reason: Option<String>,
    /// The messages drafts compare by meaning, and how many are read.
    pub wanted: i64,
    pub done: i64,
}

#[tauri::command]
pub async fn get_encoder(state: State<'_, SharedState>) -> CommandResult<EncoderView> {
    let health = if state.engine.is_ready() { state.engine.call("engine.health", json!({})).await.ok() } else { None };
    Ok(encoder_view(&state.encoder_places(), &state.db, health.as_ref())?)
}

/// The view, from where the encoder's files are, the database, and what the
/// engine said of its health (none when it isn't running).
fn encoder_view(
    places: &mimic_core::encoder::Places,
    db: &mimic_core::db::Db,
    health: Option<&Value>,
) -> Result<EncoderView, mimic_core::db::DbError> {
    let offered = places.offered();
    let downloaded = offered.as_ref().is_some_and(|m| mimic_core::encoder::downloaded(&places.encoders, m));
    let encoder = health.and_then(|v| v.get("encoder"));
    let in_use = encoder.and_then(|e| e.get("semantic")).and_then(Value::as_bool) == Some(true);
    let reason = encoder.and_then(|e| e.get("reason")).and_then(Value::as_str).map(str::to_string);
    let coverage = match &offered {
        Some(m) => db.embedding_coverage(&m.id)?,
        None => Default::default(),
    };
    Ok(EncoderView {
        offered: offered.map(|m| OfferedEncoder {
            bytes: m.bytes(),
            id: m.id,
            name: m.name,
            description: m.description,
            license: m.license,
            homepage: m.homepage,
        }),
        downloaded,
        in_use,
        reason,
        wanted: coverage.wanted,
        done: coverage.done,
    })
}

/// Download the offered encoder. One download at a time.
#[tauri::command]
pub async fn download_encoder(state: State<'_, SharedState>) -> CommandResult<mimic_core::db::Job> {
    if state.encoder_places().offered().is_none() {
        return Err(CommandError::new("no_encoder", "This build has no encoder to download."));
    }
    if let Some(active) =
        state.db.list_jobs(50, true)?.into_iter().find(|j| j.kind == mimic_core::encoder::DOWNLOAD_JOB_KIND)
    {
        return Ok(active);
    }
    Ok(state.jobs.enqueue(mimic_core::encoder::DOWNLOAD_JOB_KIND, json!({}))?)
}

/// With an encoder downloaded and the engine running it, read what has not
/// been read for meaning — once, if a reading is not already waiting.
pub(crate) async fn read_for_meaning(state: &SharedState) {
    let Some(encoder) = state.encoder_places().ready() else { return };
    // Downloaded is not enough: an engine that refused the files (changed
    // since, or a build that cannot run an encoder) would leave a job that
    // can read nothing, queued again after every check.
    if !engine_runs(state, &encoder.id).await {
        return;
    }
    // Nothing unread, nothing queued: a mailbox check that brought nothing
    // leaves no job behind it.
    match state.db.count_unembedded(&encoder.id) {
        Ok(0) => return,
        Ok(_) => {}
        Err(e) => {
            tracing::warn!(target: "encoder", error = %e, "could not count what is left to read");
            return;
        }
    }
    let waiting = state
        .db
        .list_jobs(50, true)
        .map(|active| active.iter().any(|j| j.kind == mimic_core::encoder::EMBED_JOB_KIND && j.status == "queued"));
    match waiting {
        Ok(true) => {}
        Ok(false) => {
            if let Err(e) = state.jobs.enqueue(mimic_core::encoder::EMBED_JOB_KIND, json!({})) {
                tracing::warn!(target: "encoder", error = %e, "could not queue reading for meaning");
            }
        }
        Err(e) => tracing::warn!(target: "encoder", error = %e, "could not tell whether reading is queued"),
    }
}

/// Whether the engine is running `id` now.
async fn engine_runs(state: &SharedState, id: &str) -> bool {
    if !state.engine.is_ready() {
        return false;
    }
    match state.engine.call("engine.health", json!({})).await {
        Ok(health) => runs_encoder(&health, id),
        Err(e) => {
            tracing::warn!(target: "encoder", error = %e, "could not ask the engine which encoder it runs");
            false
        }
    }
}

/// What the engine said of its health names `id` as the encoder it runs.
fn runs_encoder(health: &Value, id: &str) -> bool {
    let encoder = &health["encoder"];
    encoder["semantic"].as_bool() == Some(true) && encoder["provider"].as_str() == Some(id)
}

/// A download finished: have the engine look for the encoder again, and read
/// everything with it.
pub(crate) async fn load_downloaded(state: &SharedState) {
    match state.engine.call("encoder.reload", json!({})).await {
        Ok(v) => tracing::info!(target: "encoder", encoder = %v["encoder"]["provider"], "encoder loaded"),
        Err(e) => tracing::warn!(target: "encoder", error = %e, "the engine could not look for the encoder again"),
    }
    read_for_meaning(state).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::check_fixture;

    #[test]
    fn messages_are_read_for_meaning_only_by_the_encoder_the_engine_runs() {
        let health = |provider: &str, semantic: bool| json!({"encoder": {"provider": provider, "semantic": semantic}});
        assert!(runs_encoder(&health("all-minilm-l6-v2", true), "all-minilm-l6-v2"));
        // Downloaded but refused: the engine fell back to the lexical vectors.
        assert!(!runs_encoder(&health("lexical_v1", false), "all-minilm-l6-v2"));
        // Running some other encoder than the one downloaded.
        assert!(!runs_encoder(&health("another-encoder", true), "all-minilm-l6-v2"));
        // An engine that says nothing of it.
        assert!(!runs_encoder(&json!({}), "all-minilm-l6-v2"));
    }

    #[test]
    fn the_encoder_offered_is_described_as_it_is_and_matches_its_fixture() {
        let encoders = tempfile::tempdir().unwrap();
        let places = mimic_core::encoder::Places {
            manifests: Some(std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../models/manifests")),
            encoders: encoders.path().to_path_buf(),
        };
        let db = mimic_core::db::Db::open_in_memory().unwrap();
        let health = json!({"encoder": {
            "provider": "lexical_v1",
            "semantic": false,
            "reason": "all-MiniLM-L6-v2: model.onnx has not been downloaded; using the lexical fallback"
        }});
        let view = encoder_view(&places, &db, Some(&health)).unwrap();
        let offered = view.offered.as_ref().unwrap();
        assert_eq!(offered.id, "all-minilm-l6-v2");
        assert!(offered.bytes > 90_000_000);
        assert!(!view.downloaded && !view.in_use, "nothing is downloaded until asked");
        assert!(view.reason.as_deref().unwrap().contains("lexical fallback"));
        check_fixture("encoder.json", &serde_json::to_value(&view).unwrap());

        // With no engine running there is no word from it, and in a build
        // with no manifest nothing is offered.
        assert_eq!(encoder_view(&places, &db, None).unwrap().reason, None);
        let none = mimic_core::encoder::Places { manifests: None, encoders: encoders.path().to_path_buf() };
        assert!(encoder_view(&none, &db, None).unwrap().offered.is_none());
    }
}
