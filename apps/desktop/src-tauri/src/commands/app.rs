use serde::Serialize;
use serde_json::{json, Value};
use tauri::State;

use crate::error::CommandResult;
use crate::SharedState;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub version: String,
    pub engine_protocol_version: u32,
    pub analysis_version: String,
    pub data_root: String,
    pub started_at: String,
    pub schema_version: i64,
    pub dev_mode: bool,
    pub demo_mode: bool,
    pub os: String,
}

#[tauri::command]
pub async fn get_app_info(state: State<'_, SharedState>) -> CommandResult<AppInfo> {
    Ok(AppInfo {
        version: mimic_core::APP_VERSION.into(),
        engine_protocol_version: mimic_core::ENGINE_PROTOCOL_VERSION,
        analysis_version: mimic_core::version::ANALYSIS_VERSION.into(),
        data_root: state.paths.root.to_string_lossy().to_string(),
        started_at: state.started_at.clone(),
        schema_version: state.db.schema_version()?,
        dev_mode: state.repo_root.is_some(),
        demo_mode: state.demo(),
        os: std::env::consts::OS.into(),
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemStatus {
    pub engine: mimic_core::engine::EngineStatus,
    pub active_jobs: Vec<mimic_core::db::Job>,
    pub update: mimic_core::db::UpdateState,
    pub counts: Value,
}

#[tauri::command]
pub async fn get_system_status(state: State<'_, SharedState>) -> CommandResult<SystemStatus> {
    let sources = state.db.list_sources()?;
    Ok(SystemStatus {
        engine: state.engine.status(),
        active_jobs: state.db.list_jobs(20, true)?,
        update: state.db.update_state()?,
        counts: json!({
            "sources": sources.len(),
            "messages": sources.iter().map(|s| s.message_count).sum::<i64>(),
            "ownMessages": state.db.count_self_messages(None, None)?,
            "people": state.db.list_participants(5000)?.len(),
            "drafts": state.db.measured_draft_outcomes()?.total,
        }),
    })
}

/// The three things that must be true before Mimic can do anything, in the
/// order the onboarding asks for them.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OnboardingState {
    pub completed: bool,
    pub has_identity: bool,
    pub has_source: bool,
    pub has_own_messages: bool,
    pub has_voice_profile: bool,
}

#[tauri::command]
pub async fn get_onboarding_state(state: State<'_, SharedState>) -> CommandResult<OnboardingState> {
    let overview = mimic_core::voice::overview(&state.db)?;
    Ok(OnboardingState {
        completed: state.db.get_setting::<bool>("onboarding.completed")?.unwrap_or(false),
        has_identity: !state.db.user_identifier_set()?.is_empty(),
        has_source: !state.db.list_sources()?.is_empty(),
        has_own_messages: overview.own_messages > 0,
        has_voice_profile: overview.profiles.iter().any(|p| p.measurable),
    })
}

#[tauri::command]
pub async fn complete_onboarding(state: State<'_, SharedState>) -> CommandResult<()> {
    state.db.set_setting("onboarding.completed", &true)?;
    Ok(())
}
