/**
 * Typed IPC: every command result is validated with the shared zod contracts.
 * A shape mismatch throws in development with the offending path instead of
 * letting the UI render undefined.
 */
import { invoke } from "@tauri-apps/api/core";
import type { z } from "zod";
import {
  AppInfo,
  ConnectorInfo,
  Dashboard,
  DeletionReport,
  DiagnosticsBundle,
  Draft,
  DraftFeedback,
  DraftOutcomes,
  EngineStatus,
  EventRow,
  GenerationContext,
  InstallGuard,
  Job,
  OnboardingState,
  Participant,
  ParticipantSummary,
  ProviderState,
  RepresentativeExample,
  Settings,
  Source,
  SystemStatus,
  UpdateState,
  UserIdentity,
  ValidationReport,
  VoiceOverview,
  VoicePreference,
  CommandError,
  type ComposeRequest,
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

const z_string = AppInfo.shape.dataRoot;

export const ipc = {
  appInfo: () => call("get_app_info", AppInfo),
  dashboard: (limit = 25) => call("get_dashboard", Dashboard, { limit }),
  startAssistDrafts: () => call("start_assist_drafts", Job),
  systemStatus: () => call("get_system_status", SystemStatus),
  onboardingState: () => call("get_onboarding_state", OnboardingState),
  completeOnboarding: () => call("complete_onboarding", z_void),

  settings: () => call("get_settings", Settings),
  setSetting: (key: keyof Settings, value: unknown) =>
    call("set_setting", Settings, { key, value }),

  connectors: () => call("list_connectors", ConnectorInfo.array()),
  sources: () => call("list_sources", Source.array()),
  validateSourceFile: (connector: string, location: string) =>
    call("validate_source_file", ValidationReport, { connector, location }),
  createSource: (args: {
    connector: string;
    name: string;
    channel: string;
    location: string | null;
  }) => call("create_source", Source, args),
  startSourceImport: (sourceId: string) => call("start_source_import", Job, { sourceId }),
  deleteSource: (sourceId: string) => call("delete_source", DeletionReport, { sourceId }),
  pickSourceFile: (title: string, extensions: string[]) =>
    call("pick_source_file", z_string.nullable(), { title, extensions }),

  userIdentity: () => call("get_user_identity", UserIdentity.nullable()),
  setUserIdentity: (displayName: string) =>
    call("set_user_identity", UserIdentity, { displayName }),
  addUserIdentifier: (kind: string, value: string) =>
    call("add_user_identifier", UserIdentity, { kind, value }),
  removeUserIdentifier: (identifierId: string) =>
    call("remove_user_identifier", UserIdentity, { identifierId }),

  people: (limit = 200) => call("list_people", ParticipantSummary.array(), { limit }),
  person: (participantId: string) => call("get_person", Participant.nullable(), { participantId }),
  setPersonRelationship: (participantId: string, relationship: string | null) =>
    call("set_person_relationship", Participant, { participantId, relationship }),
  renamePerson: (participantId: string, displayName: string) =>
    call("rename_person", Participant, { participantId, displayName }),
  previewPersonDeletion: (participantId: string) =>
    call("preview_person_deletion", DeletionReport, { participantId }),
  deletePerson: (participantId: string) => call("delete_person", DeletionReport, { participantId }),
  deleteAllCommunicationData: () => call("delete_all_communication_data", DeletionReport),

  voiceOverview: () => call("get_voice_overview", VoiceOverview),
  startVoiceAnalysis: () => call("start_voice_analysis", Job),
  voiceExamples: (layer: string, scopeKey: string, limit = 10) =>
    call("list_voice_examples", RepresentativeExample.array(), { layer, scopeKey, limit }),
  voicePreferences: (layer: string, scopeKey: string) =>
    call("list_voice_preferences", VoicePreference.array(), { layer, scopeKey }),
  setVoicePreference: (args: {
    layer: string;
    scopeKey: string;
    key: string;
    value: unknown;
    note?: string | null;
  }) => call("set_voice_preference", VoicePreference, args),
  deleteVoicePreference: (preferenceId: string) =>
    call("delete_voice_preference", z_void, { preferenceId }),

  generationContext: (request: ComposeRequest) =>
    call("preview_generation_context", GenerationContext, { request }),
  generateDraft: (request: ComposeRequest) => call("generate_draft", Draft, { request }),
  resolveDraft: (draftId: string, outcome: string, finalText: string | null) =>
    call("resolve_draft", Draft, { draftId, outcome, finalText }),
  addDraftPreference: (draftId: string, note: string) =>
    call("add_draft_preference", DraftFeedback, { draftId, note }),
  recentDrafts: (limit = 25) => call("list_recent_drafts", Draft.array(), { limit }),
  draftOutcomes: () => call("get_draft_outcomes", DraftOutcomes),

  providerState: () => call("get_provider_state", ProviderState),
  setActiveProvider: (providerId: string) => call("set_active_provider", z_void, { providerId }),
  setProviderSecret: (key: string, value: string) =>
    call("set_provider_secret", z_void, { key, value }),
  checkProvider: (providerId: string) => call("check_provider", z_void, { providerId }),

  jobs: (limit = 50, activeOnly = false) => call("list_jobs", Job.array(), { limit, activeOnly }),
  job: (jobId: string) => call("get_job", Job, { jobId }),
  cancelJob: (jobId: string) => call("cancel_job", Job, { jobId }),

  diagnostics: () => call("get_diagnostics_bundle", DiagnosticsBundle),
  recentEvents: (limit = 100, minLevel?: string) =>
    call("get_recent_events", EventRow.array(), { limit, minLevel }),
  restartEngine: () => call("restart_engine", EngineStatus),
  openLogsFolder: () => call("open_logs_folder", z_string),

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
