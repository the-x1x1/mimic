import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import type { SessionSource } from "@mimic/contracts";
import { ipc } from "@/lib/ipc";
import { qk } from "@/app/queryClient";
import { toast } from "@/state/toast";

export function useSessions() {
  return useQuery({ queryKey: qk.sessions, queryFn: ipc.sessions, refetchInterval: 5_000 });
}

export function useSessionDetail(sessionId: string | null) {
  return useQuery({
    queryKey: qk.sessionDetail(sessionId ?? ""),
    queryFn: () => ipc.sessionDetail(sessionId!),
    enabled: !!sessionId,
    refetchInterval: 3_000,
  });
}

export function useSessionPhotos(sessionId: string | null) {
  return useQuery({
    queryKey: qk.sessionPhotos(sessionId ?? ""),
    queryFn: () => ipc.sessionPhotos(sessionId!),
    enabled: !!sessionId,
    refetchInterval: 4_000,
  });
}

export function useApplyPreflight(
  sessionId: string | null,
  predictionIds?: string[],
  enabled = true,
) {
  return useQuery({
    queryKey: [...qk.applyPreflight(sessionId ?? ""), predictionIds ?? null],
    queryFn: () => ipc.applyPreflight(sessionId!, predictionIds),
    enabled: !!sessionId && enabled,
    refetchInterval: 3_000,
  });
}

export function useAppliedEdits(batchId: string | null) {
  return useQuery({
    queryKey: qk.appliedEdits(batchId ?? ""),
    queryFn: () => ipc.appliedEdits(batchId!),
    enabled: !!batchId,
  });
}

export function usePredictionDetail(predictionId: string | null) {
  return useQuery({
    queryKey: qk.prediction(predictionId ?? ""),
    queryFn: () => ipc.prediction(predictionId!),
    enabled: !!predictionId,
  });
}

function invalidateSession(qc: ReturnType<typeof useQueryClient>, sessionId: string) {
  qc.invalidateQueries({ queryKey: qk.sessions });
  qc.invalidateQueries({ queryKey: qk.sessionDetail(sessionId) });
  qc.invalidateQueries({ queryKey: qk.sessionPhotos(sessionId) });
  qc.invalidateQueries({ queryKey: qk.applyPreflight(sessionId) });
  qc.invalidateQueries({ queryKey: qk.jobs });
}

export function useCreateSession() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (args: { name: string; source: SessionSource; styleId: string | null }) =>
      ipc.createSession(args.name, args.source, args.styleId),
    onSuccess: (s) => {
      invalidateSession(qc, s.id);
      toast.info("Session created", "Ingesting photos and computing features…");
    },
  });
}

export function useDeleteSession() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => ipc.deleteSession(id),
    onSuccess: () => qc.invalidateQueries({ queryKey: qk.sessions }),
    onError: (e: Error) => toast.danger("Cannot delete session", e.message),
  });
}

export function useSetSessionStyle() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (args: { sessionId: string; styleId: string | null }) =>
      ipc.setSessionStyle(args.sessionId, args.styleId),
    onSuccess: (s) => invalidateSession(qc, s.id),
    onError: (e: Error) => toast.danger("Cannot change Style", e.message),
  });
}

export function useGroupSession() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (sessionId: string) => ipc.groupSession(sessionId),
    onSuccess: (_j, sessionId) => invalidateSession(qc, sessionId),
    onError: (e: Error) => toast.danger("Cannot group scenes", e.message),
  });
}

export function usePredictSession() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (args: { sessionId: string; styleId?: string | null; consistency?: boolean }) =>
      ipc.predictSession(args.sessionId, args.styleId, args.consistency),
    onSuccess: (_j, v) => {
      invalidateSession(qc, v.sessionId);
      toast.info("Prediction queued", "Predicted edits appear as the job progresses.");
    },
    onError: (e: Error) => toast.danger("Cannot predict", e.message),
  });
}

export function useSetPredictionReview(sessionId: string) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (args: { predictionId: string; status: "pending" | "reviewed" | "rejected" }) =>
      ipc.setPredictionReview(args.predictionId, args.status),
    onSuccess: () => invalidateSession(qc, sessionId),
    onError: (e: Error) => toast.danger("Cannot update review", e.message),
  });
}

export function useApplySession() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (args: { sessionId: string; predictionIds?: string[]; minConfidence?: number }) =>
      ipc.applySession(args.sessionId, args.predictionIds, args.minConfidence),
    onSuccess: (_j, v) => {
      invalidateSession(qc, v.sessionId);
      toast.info("Apply started", "Watch the job tray; every photo is verified by read-back.");
    },
    onError: (e: Error) => toast.danger("Apply refused", e.message),
  });
}

export function useRestoreBatch(sessionId: string) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (batchId: string) => ipc.restoreApplyBatch(batchId),
    onSuccess: () => {
      invalidateSession(qc, sessionId);
      toast.info("Restore started", "Before-values are being written back and verified.");
    },
    onError: (e: Error) => toast.danger("Cannot restore", e.message),
  });
}
