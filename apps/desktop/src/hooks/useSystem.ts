import { useEffect } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { ipc } from "@/lib/ipc";
import { events } from "@/lib/events";
import { qk } from "@/app/queryClient";
import { toast } from "@/state/toast";
import { JOB_LABELS } from "@mimic/contracts";

export function useAppInfo() {
  return useQuery({ queryKey: qk.appInfo, queryFn: ipc.appInfo, staleTime: Infinity });
}

export function useSystemStatus() {
  return useQuery({ queryKey: qk.system, queryFn: ipc.systemStatus, refetchInterval: 5_000 });
}

export function useOnboardingState() {
  return useQuery({ queryKey: qk.onboarding, queryFn: ipc.onboardingState });
}

export function useSettings() {
  return useQuery({ queryKey: qk.settings, queryFn: ipc.settings, staleTime: 60_000 });
}

/** Subscribes once to native events and keeps queries fresh. Mount in AppShell. */
export function useNativeEventBridge() {
  const qc = useQueryClient();
  useEffect(() => {
    const unsubs: Array<Promise<() => void>> = [];
    unsubs.push(
      events.onJob((ev) => {
        qc.invalidateQueries({ queryKey: qk.jobs });
        if (ev.status === "completed" || ev.status === "failed" || ev.status === "canceled") {
          qc.invalidateQueries({ queryKey: qk.libraries });
          qc.invalidateQueries({ queryKey: qk.styles });
          qc.invalidateQueries({ queryKey: qk.system });
          qc.invalidateQueries({ queryKey: ["report"] });
          qc.invalidateQueries({ queryKey: ["libraryAssets"] });
          qc.invalidateQueries({ queryKey: ["style"] });
          const label = JOB_LABELS[ev.type] ?? ev.type;
          if (ev.status === "completed") toast.success(`${label} finished`);
          else if (ev.status === "failed") toast.danger(`${label} failed`, ev.message ?? undefined);
          else toast.info(`${label} canceled`);
        }
      }),
    );
    unsubs.push(
      events.onLightroom((ev) => {
        qc.invalidateQueries({ queryKey: qk.lightroom });
        qc.invalidateQueries({ queryKey: qk.capabilities });
        qc.invalidateQueries({ queryKey: qk.system });
        if (ev.type === "connected")
          toast.success(
            "Lightroom Classic connected",
            `${ev.connection.catalogName ?? "Catalog"} · Lightroom ${ev.connection.lightroomVersion}`,
          );
        if (ev.type === "disconnected") toast.warning("Lightroom disconnected", ev.reason);
      }),
    );
    unsubs.push(events.onEngineStatus(() => qc.invalidateQueries({ queryKey: qk.system })));
    unsubs.push(
      events.onEngineEvent((ev) => {
        if (ev.event === "engine.exited")
          toast.warning("Analysis engine stopped", "Mimic will restart it automatically.");
      }),
    );
    return () => {
      for (const p of unsubs) p.then((u) => u()).catch(() => {});
    };
  }, [qc]);
}
