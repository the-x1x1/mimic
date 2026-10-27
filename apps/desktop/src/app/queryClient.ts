import { QueryClient } from "@tanstack/react-query";

export const queryClient = new QueryClient({
  defaultOptions: {
    queries: { retry: 1, refetchOnWindowFocus: false, staleTime: 5_000 },
    mutations: { retry: 0 },
  },
});

export const qk = {
  appInfo: ["appInfo"] as const,
  dashboard: ["dashboard"] as const,
  // Under "dashboard": whatever refreshes the home screen — a mailbox check,
  // someone folded into the user — refreshes the conversation read from it.
  conversation: (conversationId: string, fromMessageId: string, toward: string) =>
    ["dashboard", "conversation", conversationId, fromMessageId, toward] as const,
  // Also under "dashboard", for the same reason: someone's conversations and
  // where each stands, and any conversation read from its end.
  personConversations: (participantId: string) => ["dashboard", "person", participantId] as const,
  conversationEnd: (conversationId: string) => ["dashboard", "end", conversationId] as const,
  // And what a search found: new mail can say it too.
  search: (query: string, who: string, withPerson = "") =>
    ["dashboard", "search", query, who, withPerson] as const,
  localModel: ["localModel"] as const,
  system: ["system"] as const,
  onboarding: ["onboarding"] as const,
  settings: ["settings"] as const,
  startWithWindows: ["startWithWindows"] as const,
  connectors: ["connectors"] as const,
  sources: ["sources"] as const,
  mailSignIn: ["mailSignIn"] as const,
  signInSetup: ["signInSetup"] as const,
  mailExchangers: (domain: string) => ["mailExchangers", domain] as const,
  identity: ["identity"] as const,
  // Under "identity" so that anything refreshing the identity refreshes it.
  heldAddresses: ["identity", "held"] as const,
  sentFolderPeople: ["identity", "sentFolder"] as const,
  people: ["people"] as const,
  person: (id: string) => ["person", id] as const,
  voice: ["voice"] as const,
  // Under "voice" so that anything refreshing the voice overview after an
  // analysis refreshes the situation counts with it.
  situations: ["voice", "situations"] as const,
  situationFiling: ["voice", "situationFiling"] as const,
  encoder: ["encoder"] as const,
  learning: ["voice", "learning"] as const,
  // Under "voice", so a finished job — the measurement itself, or anything
  // that deleted mail it was measured on — refreshes it.
  evaluation: ["voice", "evaluation"] as const,
  voiceExamples: (layer: string, scope: string) => ["voiceExamples", layer, scope] as const,
  voicePreferences: (layer: string, scope: string) => ["voicePrefs", layer, scope] as const,
  generationContext: (key: string) => ["generationContext", key] as const,
  drafts: ["drafts"] as const,
  // Under "dashboard": a mailbox connected, a draft decided, anything that
  // refreshes the home screen can change who a reply goes to and through what.
  envelope: (draftId: string) => ["dashboard", "envelope", draftId] as const,
  // Under "dashboard" too: a check, a reply held or stopped, or the rules
  // saved can each change it.
  whyNotByItself: (draftId: string) => ["dashboard", "byItself", draftId] as const,
  sent: ["sent"] as const,
  autoReply: ["autoReply"] as const,
  mailboxSending: (sourceId: string) => ["sources", "sending", sourceId] as const,
  // Not under "sources": asking signs in to the mailbox, so only when shown.
  mailboxFolders: (sourceId: string) => ["mailboxFolders", sourceId] as const,
  telegramApp: ["telegramApp"] as const,
  draftOutcomes: ["draftOutcomes"] as const,
  providers: ["providers"] as const,
  providerHealth: (id: string) => ["providerHealth", id] as const,
  jobs: ["jobs"] as const,
  diagnostics: ["diagnostics"] as const,
  events: ["events"] as const,
  update: ["update"] as const,
};
