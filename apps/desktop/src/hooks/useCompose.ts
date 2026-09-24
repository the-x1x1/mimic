import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import type { Adjustment, ComposeRequest } from "@mimic/contracts";
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
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: qk.drafts });
      qc.invalidateQueries({ queryKey: qk.dashboard });
    },
    onError: (e: Error) => toast.danger("Could not write a draft", e.message),
  });
}

/** Another way of saying a draft, written to be shown beside it. */
export function useWriteAnother() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ draftId, adjustment }: { draftId: string; adjustment: Adjustment }) =>
      ipc.writeAnotherDraft(draftId, adjustment),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: qk.drafts });
      qc.invalidateQueries({ queryKey: qk.dashboard });
    },
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
    // Settled, not only succeeded: a draft decided elsewhere is refused, and
    // the screen should catch up with what was decided rather than keep
    // offering it.
    onSettled: () => {
      qc.invalidateQueries({ queryKey: qk.drafts });
      qc.invalidateQueries({ queryKey: qk.draftOutcomes });
      qc.invalidateQueries({ queryKey: qk.dashboard });
      // A sent draft is evidence for the learning loop.
      qc.invalidateQueries({ queryKey: qk.learning });
    },
  });
}

export function useDraftOutcomes() {
  return useQuery({ queryKey: qk.draftOutcomes, queryFn: ipc.draftOutcomes });
}

export function useProviderState() {
  return useQuery({ queryKey: qk.providers, queryFn: ipc.providerState });
}

export type ProviderHealth = { reachable: boolean; error: string | null };

/**
 * Does the configured model actually answer? Until this has run, the answer is
 * `undefined` rather than `true`: a local endpoint with nothing listening is
 * the ordinary first-run state, and the top bar used to show a green "Local
 * model" for it while every draft failed.
 */
export function useProviderHealth(providerId: string | undefined) {
  return useQuery<ProviderHealth>({
    queryKey: qk.providerHealth(providerId ?? ""),
    enabled: !!providerId,
    retry: false,
    refetchInterval: 30_000,
    queryFn: async () => {
      try {
        await ipc.checkProvider(providerId!);
        return { reachable: true, error: null };
      } catch (e) {
        return { reachable: false, error: (e as Error).message };
      }
    },
  });
}
