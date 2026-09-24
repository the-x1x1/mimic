import { useEffect } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { JOB_KINDS, JOB_LABELS } from "@mimic/contracts";
import { ipc } from "@/lib/ipc";
import { events } from "@/lib/events";
import { qk } from "@/app/queryClient";
import { toast } from "@/state/toast";

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
          // A finished read may have folded someone back into the user, and
          // settled an address Settings listed as still held.
          for (const key of [
            qk.sources,
            qk.identity,
            qk.people,
            ["person"],
            qk.drafts,
            qk.voice,
            qk.system,
            qk.onboarding,
            qk.dashboard,
            qk.encoder,
          ]) {
            qc.invalidateQueries({ queryKey: key });
          }
          const label = JOB_LABELS[ev.type] ?? ev.type;
          // A mailbox is checked every few minutes, and what came in is
          // measured and read for meaning after; a toast each time would be
          // noise, and a failure is shown where it matters — on the home
          // screen and against the mailbox under Your mail, as out of date on
          // How you write, and as unread under Settings.
          const quiet: string[] = [
            JOB_KINDS.checkMailbox,
            JOB_KINDS.measureVoiceChanges,
            JOB_KINDS.embedMessages,
          ];
          if (quiet.includes(ev.type)) return;
          if (ev.status === "completed") toast.success(`${label} finished`);
          else if (ev.status === "failed") toast.danger(`${label} failed`, ev.message ?? undefined);
          else toast.info(`${label} canceled`);
        }
      }),
    );
    unsubs.push(events.onEngineStatus(() => qc.invalidateQueries({ queryKey: qk.system })));
    return () => {
      for (const u of unsubs) void u.then((fn) => fn());
    };
  }, [qc]);
}
