/**
 * Background update checks through the Tauri updater plugin (signature-verified
 * by the plugin). Never installs while a job is running; installs on demand or
 * on the next restart when idle (spec §24.3).
 */
import { useCallback, useEffect, useRef, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { nextCheckDelayMs } from "@mimic/contracts";
import { ipc } from "@/lib/ipc";
import { isTauri } from "@/lib/tauri";
import { qk } from "@/app/queryClient";
import { toast } from "@/state/toast";

export interface UpdaterView {
  checking: boolean;
  available: Update | null;
  downloaded: boolean;
  progress: { downloaded: number; total: number | null } | null;
  lastError: string | null;
  checkNow: () => Promise<void>;
  install: () => Promise<void>;
}

export function useUpdater(enabled: boolean): UpdaterView {
  const qc = useQueryClient();
  const [checking, setChecking] = useState(false);
  const [available, setAvailable] = useState<Update | null>(null);
  const [downloaded, setDownloaded] = useState(false);
  const [progress, setProgress] = useState<{ downloaded: number; total: number | null } | null>(
    null,
  );
  const [lastError, setLastError] = useState<string | null>(null);
  const timer = useRef<number | null>(null);

  const checkNow = useCallback(async () => {
    if (!isTauri() || checking) return;
    setChecking(true);
    setLastError(null);
    try {
      const update = await check({ timeout: 30_000 });
      if (update) {
        setAvailable(update);
        await ipc.recordUpdateCheck({
          latestSeenVersion: update.version,
          stagedVersion: null,
          result: "available",
        });
        const auto = (await ipc.settings())["updates.automatic"];
        if (auto) {
          let total: number | null = null;
          let got = 0;
          await update.download((ev) => {
            if (ev.event === "Started") total = ev.data.contentLength ?? null;
            if (ev.event === "Progress") {
              got += ev.data.chunkLength;
              setProgress({ downloaded: got, total });
            }
          });
          setDownloaded(true);
          await ipc.recordUpdateCheck({
            latestSeenVersion: update.version,
            stagedVersion: update.version,
            result: "staged",
          });
          toast.info(
            `Mimic ${update.version} is ready`,
            "It will install when Mimic is idle and restarts.",
          );
        }
      } else {
        setAvailable(null);
        await ipc.recordUpdateCheck({ stagedVersion: null, result: "up_to_date" });
      }
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      setLastError(msg);
      // GitHub unreachable must never affect the app (spec §18).
      await ipc
        .recordUpdateCheck({ stagedVersion: null, result: "error", error: msg })
        .catch(() => {});
    } finally {
      setChecking(false);
      qc.invalidateQueries({ queryKey: qk.update });
      qc.invalidateQueries({ queryKey: qk.system });
    }
  }, [checking, qc]);

  const install = useCallback(async () => {
    if (!available) return;
    const guard = await ipc.canInstallUpdateNow();
    if (!guard.allowed) {
      toast.warning("Not installing yet", guard.reason);
      return;
    }
    try {
      if (!downloaded) await available.download();
      await available.install();
      await relaunch();
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      setLastError(msg);
      toast.danger("Update failed", msg);
      await ipc
        .recordUpdateCheck({
          latestSeenVersion: available.version,
          stagedVersion: null,
          result: "error",
          error: msg,
        })
        .catch(() => {});
    }
  }, [available, downloaded]);

  useEffect(() => {
    if (!enabled || !isTauri()) return;
    const schedule = (delay: number) => {
      timer.current = window.setTimeout(async () => {
        await checkNow();
        schedule(nextCheckDelayMs());
      }, delay);
    };
    // First check shortly after initialization has settled (spec §24.3).
    schedule(20_000);
    return () => {
      if (timer.current) window.clearTimeout(timer.current);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [enabled]);

  return { checking, available, downloaded, progress, lastError, checkNow, install };
}
