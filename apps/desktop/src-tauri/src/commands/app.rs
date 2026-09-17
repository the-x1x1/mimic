use serde::Serialize;
use serde_json::{json, Value};
use tauri::State;

use crate::error::{CommandError, CommandResult};
use crate::SharedState;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub version: String,
    pub bridge_protocol_version: u32,
    pub engine_protocol_version: u32,
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
        bridge_protocol_version: mimic_core::BRIDGE_PROTOCOL_VERSION,
        engine_protocol_version: mimic_core::ENGINE_PROTOCOL_VERSION,
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
    pub lightroom: mimic_core::bridge::BridgeStatus,
    pub active_jobs: Vec<mimic_core::db::Job>,
    pub update: mimic_core::db::UpdateState,
    pub counts: Value,
}

#[tauri::command]
pub async fn get_system_status(state: State<'_, SharedState>) -> CommandResult<SystemStatus> {
    let libraries = state.db.list_libraries()?;
    let styles = state.db.list_style_profiles()?;
    Ok(SystemStatus {
        engine: state.engine.status(),
        lightroom: state.bridge.status(),
        active_jobs: state.db.list_jobs(20, true)?,
        update: state.db.update_state()?,
        counts: json!({
            "libraries": libraries.len(),
            "styles": styles.len(),
            "assets": state.db.count_assets(None)?,
            "sessions": state.db.list_sessions(1)?.len(),
        }),
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OnboardingState {
    pub completed: bool,
    pub has_library: bool,
    pub has_style: bool,
    pub lightroom_ever_connected: bool,
}

#[tauri::command]
pub async fn get_onboarding_state(state: State<'_, SharedState>) -> CommandResult<OnboardingState> {
    Ok(OnboardingState {
        completed: state.db.get_setting::<bool>("onboarding.completed")?.unwrap_or(false),
        has_library: !state.db.list_libraries()?.is_empty(),
        has_style: !state.db.list_style_profiles()?.is_empty(),
        lightroom_ever_connected: state.db.latest_lightroom_connection()?.is_some(),
    })
}

#[tauri::command]
pub async fn complete_onboarding(state: State<'_, SharedState>) -> CommandResult<()> {
    state.db.set_setting("onboarding.completed", &true)?;
    Ok(())
}

/// Demo mode seeds a clearly-labelled synthetic library from the repository
/// fixtures so the UI can be exercised without a photographer's files. It
/// never fakes a Lightroom connection.
#[tauri::command]
pub async fn enable_demo_mode(state: State<'_, SharedState>) -> CommandResult<mimic_core::db::Library> {
    let fixtures = state
        .repo_root
        .as_ref()
        .map(|r| r.join("fixtures").join("images"))
        .or_else(|| state.resource_dir.as_ref().map(|r| r.join("fixtures").join("images")))
        .filter(|p| p.is_dir())
        .ok_or_else(|| CommandError::new("demo_unavailable", "Demo fixtures are not bundled in this build."))?;
    // Materialize a demo tree in app data: images + matching XMP sidecars.
    let demo_root = state.paths.root.join("demo").join("DEMO Library");
    std::fs::create_dir_all(&demo_root)?;
    let xmp_dir = state
        .repo_root
        .as_ref()
        .map(|r| r.join("fixtures").join("xmp"))
        .or_else(|| state.resource_dir.as_ref().map(|r| r.join("fixtures").join("xmp")));
    let pairs = [
        ("demo_landscape_sky.jpg", "simple_pv2012.xmp"),
        ("demo_lowlight_indoor.jpg", "modern_masks_unknown.xmp"),
        ("demo_backlit.jpg", "legacy_pv2010.xmp"),
        ("demo_highkey_product.tif", "simple_pv2012.xmp"),
    ];
    for (img, xmp) in pairs {
        let src = fixtures.join(img);
        if src.is_file() {
            let dest = demo_root.join(format!("DEMO_{img}"));
            if !dest.exists() {
                std::fs::copy(&src, &dest)?;
            }
            if let Some(xd) = &xmp_dir {
                let xs = xd.join(xmp);
                if xs.is_file() {
                    let stem = dest.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                    let xdest = demo_root.join(format!("{stem}.xmp"));
                    if !xdest.exists() {
                        std::fs::copy(&xs, &xdest)?;
                    }
                }
            }
        }
    }
    state.demo_mode.store(true, std::sync::atomic::Ordering::Relaxed);
    let existing = state.db.list_libraries()?.into_iter().find(|l| l.source_type == "demo");
    let library = match existing {
        Some(l) => l,
        None => state.db.create_library(
            "DEMO — synthetic sample library",
            "demo",
            Some(&demo_root.to_string_lossy()),
            None,
        )?,
    };
    state.jobs.enqueue(mimic_core::ingest::JOB_SCAN_LIBRARY, json!({"libraryId": library.id}))?;
    Ok(library)
}

#[tauri::command]
pub async fn pick_folder(app: tauri::AppHandle, title: Option<String>) -> CommandResult<Option<String>> {
    use tauri_plugin_dialog::DialogExt;
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog().file().set_title(title.unwrap_or_else(|| "Choose a folder".into())).pick_folder(move |p| {
        let _ = tx.send(p.map(|f| f.to_string()));
    });
    Ok(rx.await.unwrap_or(None))
}
