import { QueryClient } from "@tanstack/react-query";

export const queryClient = new QueryClient({
  defaultOptions: {
    queries: { retry: 1, refetchOnWindowFocus: false, staleTime: 5_000 },
    mutations: { retry: 0 },
  },
});

export const qk = {
  appInfo: ["appInfo"] as const,
  system: ["system"] as const,
  onboarding: ["onboarding"] as const,
  settings: ["settings"] as const,
  libraries: ["libraries"] as const,
  libraryAssets: (id: string) => ["libraryAssets", id] as const,
  report: (id: string) => ["report", id] as const,
  styles: ["styles"] as const,
  styleDetail: (id: string) => ["style", id] as const,
  jobs: ["jobs"] as const,
  lightroom: ["lightroom"] as const,
  pluginSetup: ["pluginSetup"] as const,
  capabilities: ["capabilities"] as const,
  diagnostics: ["diagnostics"] as const,
  events: ["events"] as const,
  update: ["update"] as const,
};
