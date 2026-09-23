import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import type { ThreadMark } from "@mimic/contracts";
import { ipc } from "@/lib/ipc";
import { qk } from "@/app/queryClient";
import { toast } from "@/state/toast";

export function useDashboard(limit = 25, showLeftOut = false) {
  return useQuery({
    queryKey: [...qk.dashboard, limit, showLeftOut],
    queryFn: () => ipc.dashboard(limit, showLeftOut),
    // Switching to the left-out view keeps the list on screen while it loads
    // rather than flashing the "looking" line.
    placeholderData: (previous) => previous,
  });
}

/**
 * Say whether a thread needs a reply. The mark is tied to the message that is
 * last in the thread now, so it lapses by itself when they write again.
 */
export function useMarkThread() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({
      conversationId,
      messageId,
      mark,
    }: {
      conversationId: string;
      messageId: string;
      mark: ThreadMark | null;
    }) => ipc.markThread(conversationId, messageId, mark),
    onSuccess: (applied) => {
      void qc.invalidateQueries({ queryKey: qk.dashboard });
      if (!applied) {
        toast.info(
          "Something new came in on that thread.",
          "What you said was about the message before it, so the thread goes wherever the new one puts it.",
        );
      }
    },
    onError: (e: Error) => toast.danger("I couldn't change that", e.message),
  });
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
