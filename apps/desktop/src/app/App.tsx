import { useEffect } from "react";
import { QueryClientProvider } from "@tanstack/react-query";
import { HashRouter, Navigate, Route, Routes } from "react-router-dom";
import { Button, EmptyState } from "@mimic/ui";
import { queryClient } from "./queryClient";
import { ErrorBoundary } from "./ErrorBoundary";
import { AppShell } from "@/components/AppShell";
import { DashboardPage } from "@/features/dashboard/DashboardPage";
import { PeoplePage } from "@/features/people/PeoplePage";
import { VoicePage } from "@/features/voice/VoicePage";
import { SourcesPage } from "@/features/sources/SourcesPage";
import { SettingsPage } from "@/features/settings/SettingsPage";
import { OnboardingFlow } from "@/features/onboarding/OnboardingFlow";
import { useNativeEventBridge, useOnboardingState, useSettings } from "@/hooks/useSystem";
import { useUpdater } from "@/features/updater/useUpdater";
import { isTauri } from "@/lib/tauri";

function ThemeSync() {
  const settings = useSettings();
  useEffect(() => {
    document.documentElement.dataset.theme = settings.data?.["general.theme"] ?? "plain";
  }, [settings.data]);
  return null;
}

/**
 * Native events are subscribed above the gate, not inside AppShell: onboarding
 * runs outside the shell, and it is the screen that most needs to know when an
 * import or an analysis has finished. Mounted here it used to sit on
 * "Importing…" until the app was restarted.
 */
function NativeEvents() {
  useNativeEventBridge();
  return null;
}

function BackgroundUpdater() {
  const settings = useSettings();
  useUpdater(settings.data?.["updates.automatic"] ?? true);
  return null;
}

function Gate() {
  const onboarding = useOnboardingState();
  if (onboarding.isError) {
    return (
      <div className="fatal">
        <EmptyState
          title="Mimic could not reach its native core"
          body={(onboarding.error as Error).message}
          primary={
            <Button variant="primary" onClick={() => onboarding.refetch()}>
              Retry
            </Button>
          }
        />
      </div>
    );
  }
  if (!onboarding.data)
    return (
      <div className="splash" aria-busy="true">
        <span className="brand__mark" /> Starting Mimic…
      </div>
    );
  if (!onboarding.data.completed) {
    return (
      <Routes>
        <Route path="*" element={<OnboardingFlow />} />
      </Routes>
    );
  }
  return (
    <Routes>
      <Route element={<AppShell />}>
        <Route path="/" element={<DashboardPage />} />
        <Route path="/people" element={<PeoplePage />} />
        <Route path="/voice" element={<VoicePage />} />
        <Route path="/sources" element={<SourcesPage />} />
        <Route path="/settings" element={<SettingsPage />} />
        <Route path="*" element={<Navigate to="/" replace />} />
      </Route>
    </Routes>
  );
}

export function App() {
  if (!isTauri()) {
    return (
      <div className="fatal">
        <EmptyState
          title="Open Mimic from the desktop app"
          body="This interface talks to Mimic's native core over Tauri IPC. Run pnpm dev (tauri dev) instead of opening the Vite URL in a browser."
        />
      </div>
    );
  }
  return (
    <QueryClientProvider client={queryClient}>
      <ErrorBoundary>
        <HashRouter>
          <ThemeSync />
          <NativeEvents />
          <BackgroundUpdater />
          <Gate />
        </HashRouter>
      </ErrorBoundary>
    </QueryClientProvider>
  );
}
