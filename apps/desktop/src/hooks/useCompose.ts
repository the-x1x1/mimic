import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import type { ComposeRequest } from "@mimic/contracts";
import { ipc } from "@/lib/ipc";
import { qk } from "@/app/queryClient";
import { toast } from "@/state/toast";

/**
 * The context is fetched separately from the draft so the Compose screen can
 * say what it will base a draft on *before* the user asks for one.
 */
export function useGenerationContext(request: ComposeRequest, enabled: boolean) {
  const key = `${request.participantId ?? ""}|${request.channel}|${request.incomingMessage ?? ""}`;
  return useQuery({
    queryKey: qk.generationContext(key),
    queryFn: () => ipc.generationContext(request),
    enabled,
  });
}

export function useGenerateDraft() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ipc.generateDraft,
    onSuccess: () => qc.invalidateQueries({ queryKey: qk.drafts }),
    onError: (e: Error) => toast.danger("Could not write a draft", e.message),
  });
}

export function useResolveDraft() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({
      draftId,
      outcome,
      finalText,
    }: {
      draftId: string;
      outcome: string;
      finalText: string | null;
    }) => ipc.resolveDraft(draftId, outcome, finalText),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: qk.drafts });
      qc.invalidateQueries({ queryKey: qk.draftOutcomes });
    },
  });
}

export function useDraftOutcomes() {
  return useQuery({ queryKey: qk.draftOutcomes, queryFn: ipc.draftOutcomes });
}

export function useProviderState() {
  return useQuery({ queryKey: qk.providers, queryFn: ipc.providerState });
}
