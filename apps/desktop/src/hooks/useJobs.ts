import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ipc } from "@/lib/ipc";
import { qk } from "@/app/queryClient";
import { toast } from "@/state/toast";

export function useJobs(activeOnly = false) {
  return useQuery({
    queryKey: [...qk.jobs, activeOnly],
    queryFn: () => ipc.jobs(50, activeOnly),
    refetchInterval: activeOnly ? 2_000 : 10_000,
  });
}

export function useCancelJob() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (jobId: string) => ipc.cancelJob(jobId),
    onSuccess: () => qc.invalidateQueries({ queryKey: qk.jobs }),
    onError: (e: Error) => toast.danger("Could not cancel job", e.message),
  });
}
