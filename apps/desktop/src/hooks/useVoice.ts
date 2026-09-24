import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ipc } from "@/lib/ipc";
import { qk } from "@/app/queryClient";
import { toast } from "@/state/toast";

export function useVoiceOverview() {
  return useQuery({ queryKey: qk.voice, queryFn: ipc.voiceOverview });
}

export function useSituations() {
  return useQuery({ queryKey: qk.situations, queryFn: ipc.situations, staleTime: 60_000 });
}

/** How your messages came to be filed by what they are doing, and the model on this computer, if any. */
export function useSituationFiling() {
  return useQuery({ queryKey: qk.situationFiling, queryFn: ipc.situationFiling });
}

export function useStartReadingSituations() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ipc.startReadingSituations,
    onSuccess: () => qc.invalidateQueries({ queryKey: qk.jobs }),
    onError: (e: Error) => toast.danger("I couldn't start reading them", e.message),
  });
}

/**
 * Say what one of your messages is doing, or hand it back to the rules
 * (`situationIds: null`). Resolves to how it is filed now.
 */
export function useFileMessage() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({
      messageId,
      situationIds,
    }: {
      messageId: string;
      situationIds: string[] | null;
    }) =>
      situationIds === null
        ? ipc.letRulesDecide(messageId)
        : ipc.decideSituations(messageId, situationIds),
    // The counts, what each layer is measured over, and the conversations
    // the message is shown in have all moved.
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: qk.voice });
      qc.invalidateQueries({ queryKey: qk.dashboard });
    },
    onError: (e: Error) => toast.danger("I couldn't keep that", e.message),
  });
}

export function useLearning() {
  return useQuery({ queryKey: qk.learning, queryFn: ipc.learning });
}

export function useAddVoiceNote() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ participantId, note }: { participantId: string | null; note: string }) =>
      ipc.addVoiceNote(participantId, note),
    onSuccess: () => qc.invalidateQueries({ queryKey: qk.learning }),
    onError: (e: Error) => toast.danger("I couldn't keep that", e.message),
  });
}

export function useForgetVoiceNote() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ipc.deleteVoicePreference,
    onSuccess: () => qc.invalidateQueries({ queryKey: qk.learning }),
    onError: (e: Error) => toast.danger("I couldn't forget that", e.message),
  });
}

export function useStartAnalysis() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ipc.startVoiceAnalysis,
    onSuccess: () => qc.invalidateQueries({ queryKey: qk.jobs }),
    onError: (e: Error) => toast.danger("Could not start the analysis", e.message),
  });
}

export function useStartDescribing() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ipc.startDescribingVoice,
    onSuccess: () => qc.invalidateQueries({ queryKey: qk.jobs }),
    onError: (e: Error) => toast.danger("I couldn't start putting it into words", e.message),
  });
}

export function useVoiceExamples(layer: string, scopeKey: string, enabled = true) {
  return useQuery({
    queryKey: qk.voiceExamples(layer, scopeKey),
    queryFn: () => ipc.voiceExamples(layer, scopeKey),
    enabled,
  });
}

export function useVoicePreferences(layer: string, scopeKey: string) {
  return useQuery({
    queryKey: qk.voicePreferences(layer, scopeKey),
    queryFn: () => ipc.voicePreferences(layer, scopeKey),
  });
}

export function useSetVoicePreference(layer: string, scopeKey: string) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (args: { key: string; value: unknown; note?: string | null }) =>
      ipc.setVoicePreference({ layer, scopeKey, ...args }),
    onSuccess: () => qc.invalidateQueries({ queryKey: qk.voicePreferences(layer, scopeKey) }),
    onError: (e: Error) => toast.danger("Could not save that preference", e.message),
  });
}

export function useDeleteVoicePreference(layer: string, scopeKey: string) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ipc.deleteVoicePreference,
    onSuccess: () => qc.invalidateQueries({ queryKey: qk.voicePreferences(layer, scopeKey) }),
  });
}

/** The latest measurement of the drafts against what you wrote. */
export function useEvaluation() {
  return useQuery({ queryKey: qk.evaluation, queryFn: ipc.evaluation });
}

export function useStartEvaluation() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: () => ipc.startEvaluation(),
    onSuccess: () => qc.invalidateQueries({ queryKey: qk.jobs }),
  });
}

/** The sentence encoder: offered, downloaded, in use, and how much it has read. */
export function useEncoder() {
  return useQuery({ queryKey: qk.encoder, queryFn: ipc.encoder });
}

export function useDownloadEncoder() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ipc.downloadEncoder,
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: qk.jobs });
      qc.invalidateQueries({ queryKey: qk.encoder });
    },
    onError: (e: Error) => toast.danger("I couldn't start the download", e.message),
  });
}
