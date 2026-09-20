import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ipc } from "@/lib/ipc";
import { qk } from "@/app/queryClient";
import { toast } from "@/state/toast";

export function useVoiceOverview() {
  return useQuery({ queryKey: qk.voice, queryFn: ipc.voiceOverview });
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
