/**
 * Typed IPC: every command result is validated with the shared zod contracts.
 * A shape mismatch throws in development with the offending path instead of
 * letting the UI render undefined.
 */
import { invoke } from "@tauri-apps/api/core";
import { convertFileSrc } from "@tauri-apps/api/core";
import type { z } from "zod";
import {
  AppInfo,
  AppliedEdit,
  ApplyPreflight,
  AssetDetail,
  AssetRow,
  CapabilityMatrixResponse,
  CommandError,
  ConnectionTest,
  CorrectionRow,
  StyleHealth,
  DataQualityReport,
  DiagnosticsBundle,
  EngineStatus,
  EventRow,
  InstallGuard,
  Job,
  Library,
  LibrarySummary,
  LightroomStatus,
  ModelVersion,
  OnboardingState,
  PluginSetup,
  Prediction,
  PredictionDetail,
  Session,
  SessionDetail,
  SessionPhoto,
  type SessionSource,
  Settings,
  StyleDetail,
  StyleSummary,
  SystemStatus,
  UpdateState,
} from "@mimic/contracts";
import { isTauri } from "./tauri";

export class IpcError extends Error {
  code: string;
  constructor(code: string, message: string) {
    super(message);
    this.code = code;
    this.name = "IpcError";
  }
}

async function call<T>(
  command: string,
  schema: z.ZodType<T>,
  args?: Record<string, unknown>,
): Promise<T> {
  if (!isTauri()) {
    throw new IpcError(
      "not_in_tauri",
      `Command ${command} is only available inside the Mimic desktop app.`,
    );
  }
  let raw: unknown;
  try {
    raw = await invoke(command, args);
  } catch (e) {
    const parsed = CommandError.safeParse(e);
    if (parsed.success) throw new IpcError(parsed.data.code, parsed.data.message);
    throw new IpcError("unknown", typeof e === "string" ? e : ((e as Error)?.message ?? String(e)));
  }
  const result = schema.safeParse(raw);
  if (!result.success) {
    console.error(`[ipc] ${command} returned an unexpected shape`, result.error.issues, raw);
    throw new IpcError(
      "bad_shape",
      `${command}: ${result.error.issues[0]?.path.join(".")} ${result.error.issues[0]?.message}`,
    );
  }
  return result.data;
}

const z_void = {
  safeParse: () => ({ success: true as const, data: undefined }),
} as unknown as z.ZodType<void>;

export const ipc = {
  appInfo: () => call("get_app_info", AppInfo),
  systemStatus: () => call("get_system_status", SystemStatus),
  onboardingState: () => call("get_onboarding_state", OnboardingState),
  completeOnboarding: () => call("complete_onboarding", z_void),
  enableDemoMode: () => call("enable_demo_mode", Library),
  pickFolder: (title?: string) => call("pick_folder", AppInfo.shape.dataRoot.nullable(), { title }),

  settings: () => call("get_settings", Settings),
  setSetting: (key: keyof Settings, value: unknown) =>
    call("set_setting", Settings, { key, value }),

  libraries: () => call("list_libraries", LibrarySummary.array()),
  createLibrary: (name: string, sourceType: string, rootPath: string | null) =>
    call("create_library", Library, { name, sourceType, rootPath }),
  deleteLibrary: (libraryId: string) => call("delete_library", z_void, { libraryId }),
  startLibraryScan: (libraryId: string) => call("start_library_scan", Job, { libraryId }),
  dataQualityReport: (libraryId: string) =>
    call("get_data_quality_report", DataQualityReport, { libraryId }),
  libraryAssets: (libraryId: string, limit = 200, offset = 0) =>
    call("list_library_assets", AssetRow.array(), { libraryId, limit, offset }),
  assetDetail: (assetId: string) => call("get_asset_detail", AssetDetail, { assetId }),

  styles: () => call("list_styles", StyleSummary.array()),
  createStyle: (name: string, description: string | null, libraryId: string | null) =>
    call("create_style", StyleSummary, { name, description, libraryId }),
  deleteStyle: (styleId: string) => call("delete_style", z_void, { styleId }),
  attachLibraryToStyle: (styleId: string, libraryId: string) =>
    call("attach_library_to_style", StyleSummary, { styleId, libraryId }),
  styleDetail: (styleId: string) => call("get_style_detail", StyleDetail, { styleId }),
  trainStyle: (styleId: string, config?: Record<string, unknown>) =>
    call("train_style", Job, { styleId, config }),
  activateModelVersion: (modelVersionId: string) =>
    call("activate_model_version", ModelVersion, { modelVersionId }),
  archiveModelVersion: (modelVersionId: string) =>
    call("archive_model_version", ModelVersion, { modelVersionId }),
  modelVersion: (modelVersionId: string) =>
    call("get_model_version", ModelVersion, { modelVersionId }),
  styleHealth: (styleId: string) => call("get_style_health", StyleHealth, { styleId }),
  corrections: (styleId: string, limit = 200) =>
    call("list_corrections", CorrectionRow.array(), { styleId, limit }),

  sessions: () => call("list_sessions", Session.array()),
  createSession: (name: string, source: SessionSource, styleId: string | null) =>
    call("create_session", Session, { name, source, styleId }),
  sessionDetail: (sessionId: string) => call("get_session_detail", SessionDetail, { sessionId }),
  sessionPhotos: (sessionId: string) =>
    call("list_session_photos", SessionPhoto.array(), { sessionId }),
  setSessionStyle: (sessionId: string, styleId: string | null) =>
    call("set_session_style", Session, { sessionId, styleId }),
  deleteSession: (sessionId: string) => call("delete_session", z_void, { sessionId }),
  groupSession: (sessionId: string) => call("group_session", Job, { sessionId }),
  predictSession: (sessionId: string, styleId?: string | null, consistency?: boolean) =>
    call("predict_session", Job, { sessionId, styleId: styleId ?? null, consistency }),
  setPredictionReview: (predictionId: string, status: "pending" | "reviewed" | "rejected") =>
    call("set_prediction_review", Prediction, { predictionId, status }),
  applyPreflight: (sessionId: string, predictionIds?: string[]) =>
    call("get_apply_preflight", ApplyPreflight, { sessionId, predictionIds }),
  applySession: (sessionId: string, predictionIds?: string[], minConfidence?: number) =>
    call("apply_session", Job, { sessionId, predictionIds, minConfidence }),
  appliedEdits: (applyBatchId: string) =>
    call("list_applied_edits", AppliedEdit.array(), { applyBatchId }),
  restoreApplyBatch: (applyBatchId: string) => call("restore_apply_batch", Job, { applyBatchId }),
  prediction: (predictionId: string) => call("get_prediction", PredictionDetail, { predictionId }),
  syncCorrections: (sessionId: string) => call("sync_corrections", Job, { sessionId }),

  jobs: (limit = 50, activeOnly = false) => call("list_jobs", Job.array(), { limit, activeOnly }),
  job: (jobId: string) => call("get_job", Job, { jobId }),
  cancelJob: (jobId: string) => call("cancel_job", Job, { jobId }),

  lightroomStatus: () => call("get_lightroom_status", LightroomStatus),
  pluginSetup: () => call("get_plugin_setup", PluginSetup),
  installPlugin: () => call("install_lightroom_plugin", PluginSetup),
  testLightroom: () => call("test_lightroom_connection", ConnectionTest),
  startLightroomIngest: (args: { libraryId?: string; name?: string; scope?: string }) =>
    call("start_lightroom_ingest", Job, args),
  capabilityMatrix: () => call("get_capability_matrix", CapabilityMatrixResponse),

  diagnostics: () => call("get_diagnostics_bundle", DiagnosticsBundle),
  recentEvents: (limit = 100, minLevel?: string) =>
    call("get_recent_events", EventRow.array(), { limit, minLevel }),
  restartEngine: () => call("restart_engine", EngineStatus),
  openLogsFolder: () => call("open_logs_folder", AppInfo.shape.dataRoot),

  updateState: () => call("get_update_state", UpdateState),
  setUpdatePreferences: (prefs: { channel?: string; automatic?: boolean }) =>
    call("set_update_preferences", UpdateState, { prefs }),
  recordUpdateCheck: (record: {
    latestSeenVersion?: string | null;
    stagedVersion?: string | null;
    result: string;
    error?: string | null;
  }) => call("record_update_check", UpdateState, { record }),
  canInstallUpdateNow: () => call("can_install_update_now", InstallGuard),
};

/** Turn a cached preview path into a URL the webview may load (asset protocol). */
export function previewUrl(path: string | null | undefined): string | undefined {
  if (!path || !isTauri()) return undefined;
  return convertFileSrc(path);
}
