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
