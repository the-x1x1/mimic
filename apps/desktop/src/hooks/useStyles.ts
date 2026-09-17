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
  });
}
