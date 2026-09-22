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
  localModel: ["localModel"] as const,
  system: ["system"] as const,
  onboarding: ["onboarding"] as const,
  settings: ["settings"] as const,
  connectors: ["connectors"] as const,
  sources: ["sources"] as const,
  identity: ["identity"] as const,
  people: ["people"] as const,
  person: (id: string) => ["person", id] as const,
  voice: ["voice"] as const,
  // Under "voice" so that anything refreshing the voice overview after an
  // analysis refreshes the situation counts with it.
  situations: ["voice", "situations"] as const,
  voiceExamples: (layer: string, scope: string) => ["voiceExamples", layer, scope] as const,
  voicePreferences: (layer: string, scope: string) => ["voicePrefs", layer, scope] as const,
  generationContext: (key: string) => ["generationContext", key] as const,
  drafts: ["drafts"] as const,
  draftOutcomes: ["draftOutcomes"] as const,
  providers: ["providers"] as const,
  providerHealth: (id: string) => ["providerHealth", id] as const,
  jobs: ["jobs"] as const,
  diagnostics: ["diagnostics"] as const,
  events: ["events"] as const,
  update: ["update"] as const,
};
