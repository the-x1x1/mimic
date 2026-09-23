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
