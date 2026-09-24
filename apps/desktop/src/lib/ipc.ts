/**
 * Typed IPC: every command result is validated with the shared zod contracts.
 * A shape mismatch throws in development with the offending path instead of
 * letting the UI render undefined.
 */
import { invoke } from "@tauri-apps/api/core";
import type { z } from "zod";
import {
  AddressAdded,
  AddressPreview,
  HeldAddress,
  type AddressOwner,
  AppInfo,
  ConnectorInfo,
  ConversationPage,
  PersonConversations,
  Dashboard,
  DeletionReport,
  DiagnosticsBundle,
  Draft,
  DraftFeedback,
  DraftOutcomes,
  EngineStatus,
  EvaluationView,
  EventRow,
  GenerationContext,
  InstallGuard,
  ImapProbe,
  Job,
  LearningOverview,
  LocalModelStatus,
  OnboardingState,
  Participant,
  PeopleView,
  ProviderState,
  RepresentativeExample,
  SentFolderPerson,
  Settings,
  SituationFiling,
  EncoderView,
  SituationSummary,
  Filing,
  Source,
  SystemStatus,
  UpdateState,
  UserIdentity,
  ValidationReport,
  VoiceOverview,
  VoicePreference,
  CommandError,
  type ComposeRequest,
  type ImapAccount,
  type ThreadMark,
  type Toward,
} from "@mimic/contracts";
import { isTauri } from "./tauri";
import { qk, queryClient } from "@/app/queryClient";

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
const z_bool = Dashboard.shape.showingLeftOut;

export const ipc = {
  appInfo: () => call("get_app_info", AppInfo),
  dashboard: (limit = 25, showLeftOut = false) =>
    call("get_dashboard", Dashboard, { limit, showLeftOut }),
  /**
   * Up to `limit` messages of a conversation next to one of its messages,
   * toward its start or its end, oldest first.
   */
  conversationPage: (conversationId: string, fromMessageId: string, toward: Toward, limit = 20) =>
    call("get_conversation_page", ConversationPage, {
      conversationId,
      fromMessageId,
      toward,
      limit,
    }),
  /** The last messages of any conversation, oldest first: where reading it starts. */
  conversationEnd: (conversationId: string, limit = 20) =>
    call("get_conversation_end", ConversationPage, { conversationId, limit }),
  /**
   * Every conversation someone is in, most recently active first, with where
   * each stands; read on from the last one shown.
   */
  personConversations: (
    participantId: string,
    before: { at: string; id: string } | null = null,
    limit = 20,
  ) =>
    call("list_person_conversations", PersonConversations, {
      participantId,
      beforeAt: before?.at ?? null,
      beforeId: before?.id ?? null,
      limit,
    }),
  /**
   * Say a thread does or does not need a reply, about the message on screen;
   * null takes it back. Resolves to whether it applies — false when someone
   * wrote again in the meantime.
   */
  markThread: (conversationId: string, messageId: string, mark: ThreadMark | null) =>
    call("mark_thread", z_bool, { conversationId, messageId, mark }),
  startAssistDrafts: () => call("start_assist_drafts", Job),
  localModelStatus: () => call("local_model_status", LocalModelStatus),
  startModelPull: () => call("start_model_pull", Job),
  systemStatus: () => call("get_system_status", SystemStatus),
  onboardingState: () => call("get_onboarding_state", OnboardingState),
  completeOnboarding: () => call("complete_onboarding", z_void),

  settings: () => call("get_settings", Settings),
  /**
   * Save one setting and put the answer straight into the cache. Every control
   * on the Settings screen reads from that cache, so without this a checkbox
   * or a theme swatch snapped back to its old value until something happened
   * to refetch — the save had worked and the screen said it had not.
   */
  setSetting: async (key: keyof Settings, value: unknown) => {
    const next = await call("set_setting", Settings, { key, value });
    queryClient.setQueryData(qk.settings, next);
    // The home screen reads some settings too (whether replies are prepared
    // in advance, how often mail is checked).
    void queryClient.invalidateQueries({ queryKey: qk.dashboard });
    return next;
  },

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
  probeMailbox: (account: ImapAccount, password: string) =>
    call("probe_mailbox", ImapProbe, { account, password }),
  connectMailbox: (account: ImapAccount, password: string, isMine: boolean) =>
    call("connect_mailbox", Source, { account, password, isMine }),
  /**
   * Give a connected mailbox its password again, keeping everything imported
   * through it. The login is tried first; nothing is saved if it fails.
   */
  setMailboxPassword: (sourceId: string, password: string) =>
    call("set_mailbox_password", Source, { sourceId, password }),
  /** Whether this copy of Mimic can sign in to a mailbox with Microsoft. */
  mailSignInAvailable: () => call("mail_sign_in_available", z_bool),
  /**
   * Sign in with Microsoft in the browser, and look at the mailbox without
   * importing anything. Resolves once the browser has come back.
   */
  signInToMailbox: (email: string) => call("sign_in_to_mailbox", ImapProbe, { email }),
  /** Connect the mailbox just signed in to. */
  connectSignedInMailbox: (email: string, isMine: boolean) =>
    call("connect_signed_in_mailbox", Source, { email, isMine }),
  /** Stop a sign-in waiting on the browser, and forget one not yet connected. */
  cancelMailSignIn: () => call("cancel_mail_sign_in", z_void),
  /** Sign a connected Microsoft mailbox in again, keeping everything read from it. */
  signInMailboxAgain: (sourceId: string) => call("sign_in_mailbox_again", Source, { sourceId }),
  deleteSource: (sourceId: string) => call("delete_source", DeletionReport, { sourceId }),
  pickSourceFile: (title: string, extensions: string[]) =>
    call("pick_source_file", z_string.nullable(), { title, extensions }),

  userIdentity: () => call("get_user_identity", UserIdentity.nullable()),
  setUserIdentity: (displayName: string) =>
    call("set_user_identity", UserIdentity, { displayName }),
  /** What adding an address would do. Changes nothing. */
  previewUserAddress: (kind: string, value: string) =>
    call("preview_user_address", AddressPreview, { kind, value }),
  /**
   * Add an address. When the preview named someone the mail from it is filed
   * under, `confirmedOwner` must be that preview's owner, exactly as it still
   * is, or nothing changes (code `confirm`).
   */
  addUserIdentifier: (kind: string, value: string, confirmedOwner: AddressOwner | null = null) =>
    call("add_user_identifier", AddressAdded, { kind, value, confirmedOwner }),
  heldUserAddresses: () => call("held_user_addresses", HeldAddress.array()),
  /** People whose mail was in the user's Sent folder, under an address not yet theirs. */
  sentFolderPeople: () => call("sent_folder_people", SentFolderPerson.array()),
  /** Fold in the holder of a held address, when `confirmedOwner` is what the list shows now. */
  claimHeldAddress: (identifierId: string, confirmedOwner: AddressOwner) =>
    call("claim_held_address", AddressAdded, { identifierId, confirmedOwner }),
  /** The person under one of the user's addresses is not them: nothing moves, and it is remembered. */
  keepPersonApart: (participantId: string) => call("keep_person_apart", z_void, { participantId }),
  removeUserIdentifier: (identifierId: string) =>
    call("remove_user_identifier", UserIdentity, { identifierId }),

  /**
   * Up to `limit` people, with the counts; the senders whose mail all looks
   * automated only when `automatedLimit` asks for them. Pass a `limit` of 0
   * to get only the senders.
   */
  people: (limit = 200, automatedLimit: number | null = null) =>
    call("list_people", PeopleView, { limit, automatedLimit }),
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
  situations: () => call("list_situations", SituationSummary.array()),
  situationFiling: () => call("get_situation_filing", SituationFiling),
  encoder: () => call("get_encoder", EncoderView),
  /** Download the sentence encoder this build offers; one download at a time. */
  downloadEncoder: () => call("download_encoder", Job),
  /** Have the model on this computer read what your messages are doing; refused without one. */
  startReadingSituations: () => call("start_reading_situations", Job),
  /** Say what one of your messages is doing: these situations, or none. */
  decideSituations: (messageId: string, situationIds: string[]) =>
    call("decide_situations", Filing, { messageId, situationIds }),
  letRulesDecide: (messageId: string) => call("let_rules_decide", Filing, { messageId }),
  learning: () => call("get_learning_overview", LearningOverview),
  addVoiceNote: (participantId: string | null, note: string) =>
    call("add_voice_note", VoicePreference, { participantId, note }),
  startVoiceAnalysis: () => call("start_voice_analysis", Job),
  /** Have the chosen provider put each measured layer into words, from its numbers alone. */
  startDescribingVoice: () => call("start_describing_voice", Job),
  /** The latest measurement of the drafts, from the cases still here; null before the first. */
  evaluation: () => call("get_evaluation", EvaluationView.nullable()),
  startEvaluation: () => call("start_evaluation", Job),
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
