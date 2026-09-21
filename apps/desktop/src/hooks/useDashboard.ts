import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ipc } from "@/lib/ipc";
import { qk } from "@/app/queryClient";
import { toast } from "@/state/toast";

export function useDashboard(limit = 25) {
  return useQuery({ queryKey: [...qk.dashboard, limit], queryFn: () => ipc.dashboard(limit) });
}

/**
 * Queue a run of assisted drafting. The command refuses when the setting is
 * off, so this cannot quietly do nothing.
 */
export function useStartAssistDrafts() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ipc.startAssistDrafts,
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: qk.jobs });
      toast.info("Preparing replies", "Mimic is drafting for the threads that are waiting.");
    },
    onError: (e: Error) => toast.danger("Could not prepare replies", e.message),
  });
}

/**
 * What is on this computer to write with. Polled while a download is running,
 * because the answer changes underneath the screen — and polled slowly the
 * rest of the time, because it reaches out to a local HTTP server each call.
 */
export function useLocalModel(active = false) {
  return useQuery({
    queryKey: qk.localModel,
    queryFn: ipc.localModelStatus,
    refetchInterval: active ? 3_000 : false,
    staleTime: 5_000,
  });
}

/** Start the download. Refuses when there is nothing to download into. */
export function useStartModelPull() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ipc.startModelPull,
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: qk.jobs });
      qc.invalidateQueries({ queryKey: qk.localModel });
    },
    onError: (e: Error) => toast.danger("Could not start the download", e.message),
  });
}
