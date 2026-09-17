import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ipc } from "@/lib/ipc";
import { qk } from "@/app/queryClient";
import { toast } from "@/state/toast";

export function useStyles() {
  return useQuery({ queryKey: qk.styles, queryFn: ipc.styles });
}

export function useStyleDetail(styleId: string | null) {
  return useQuery({
    queryKey: qk.styleDetail(styleId ?? ""),
    queryFn: () => ipc.styleDetail(styleId!),
    enabled: !!styleId,
    refetchInterval: 5_000,
  });
}

export function useCreateStyle() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (args: { name: string; description: string | null; libraryId: string | null }) =>
      ipc.createStyle(args.name, args.description, args.libraryId),
    onSuccess: () => qc.invalidateQueries({ queryKey: qk.styles }),
    onError: (e: Error) => toast.danger("Could not create Style", e.message),
  });
}

export function useAttachLibrary() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (args: { styleId: string; libraryId: string }) =>
      ipc.attachLibraryToStyle(args.styleId, args.libraryId),
    onSuccess: (_d, v) => {
      qc.invalidateQueries({ queryKey: qk.styles });
      qc.invalidateQueries({ queryKey: qk.styleDetail(v.styleId) });
    },
  });
}

export function useDeleteStyle() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => ipc.deleteStyle(id),
    onSuccess: () => qc.invalidateQueries({ queryKey: qk.styles }),
    onError: (e: Error) => toast.danger("Cannot delete Style", e.message),
  });
}

export function useTrainStyle() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (args: { styleId: string; config?: Record<string, unknown> }) =>
      ipc.trainStyle(args.styleId, args.config),
    onSuccess: (_j, v) => {
      qc.invalidateQueries({ queryKey: qk.jobs });
      qc.invalidateQueries({ queryKey: qk.styleDetail(v.styleId) });
      toast.info("Training queued", "A new immutable version is being trained.");
    },
    onError: (e: Error) => toast.danger("Cannot train", e.message),
  });
}

export function useActivateVersion() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => ipc.activateModelVersion(id),
    onSuccess: (mv) => {
      qc.invalidateQueries({ queryKey: qk.styles });
      qc.invalidateQueries({ queryKey: qk.styleDetail(mv.styleProfileId) });
      toast.success(`v${mv.semanticVersion} is now active`);
    },
    onError: (e: Error) => toast.danger("Cannot activate", e.message),
  });
}

export function useArchiveVersion() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => ipc.archiveModelVersion(id),
    onSuccess: (mv) => qc.invalidateQueries({ queryKey: qk.styleDetail(mv.styleProfileId) }),
    onError: (e: Error) => toast.danger("Cannot archive", e.message),
  });
}

export function useCorrections(styleId: string | null) {
  return useQuery({
    queryKey: qk.corrections(styleId ?? ""),
    queryFn: () => ipc.corrections(styleId!),
    enabled: !!styleId,
  });
}
