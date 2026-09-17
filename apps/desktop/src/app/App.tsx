import { QueryClientProvider } from "@tanstack/react-query";
import { HashRouter, Navigate, Route, Routes } from "react-router-dom";
import { Button, EmptyState } from "@mimic/ui";
import { queryClient } from "./queryClient";
import { ErrorBoundary } from "./ErrorBoundary";
import { AppShell } from "@/components/AppShell";
import { HomePage } from "@/features/home/HomePage";
import { StylesPage } from "@/features/styles/StylesPage";
import { StyleDetailPage } from "@/features/styles/StyleDetailPage";
import { SessionsPage } from "@/features/sessions/SessionsPage";
import { SessionDetailPage } from "@/features/sessions/SessionDetailPage";
import { ReviewPage } from "@/features/review/ReviewPage";
import { SettingsPage } from "@/features/settings/SettingsPage";
import { OnboardingFlow } from "@/features/onboarding/OnboardingFlow";
import { useOnboardingState, useSettings } from "@/hooks/useSystem";
import { useUpdater } from "@/features/updater/useUpdater";
import { isTauri } from "@/lib/tauri";
import { useEffect } from "react";

function ThemeSync() {
  const settings = useSettings();
  useEffect(() => {
    document.documentElement.dataset.theme = settings.data?.["general.theme"] ?? "dark";
  }, [settings.data]);
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
        <Route path="/" element={<HomePage />} />
        <Route path="/styles" element={<StylesPage />} />
        <Route path="/styles/:styleId" element={<StyleDetailPage />} />
        <Route path="/sessions" element={<SessionsPage />} />
        <Route path="/sessions/:sessionId" element={<SessionDetailPage />} />
        <Route path="/review" element={<ReviewPage />} />
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
          <BackgroundUpdater />
          <Gate />
        </HashRouter>
      </ErrorBoundary>
    </QueryClientProvider>
  );
}
