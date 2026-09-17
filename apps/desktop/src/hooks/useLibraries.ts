import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ipc } from "@/lib/ipc";
import { qk } from "@/app/queryClient";
import { toast } from "@/state/toast";

export function useLibraries() {
  return useQuery({ queryKey: qk.libraries, queryFn: ipc.libraries });
}

export function useDataQualityReport(libraryId: string | null) {
  return useQuery({
    queryKey: qk.report(libraryId ?? ""),
    queryFn: () => ipc.dataQualityReport(libraryId!),
    enabled: !!libraryId,
    refetchInterval: 5_000,
  });
}

export function useLibraryAssets(libraryId: string | null) {
  return useQuery({
    queryKey: qk.libraryAssets(libraryId ?? ""),
    queryFn: () => ipc.libraryAssets(libraryId!, 400, 0),
    enabled: !!libraryId,
  });
}

export function useCreateLibrary() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (args: { name: string; sourceType: string; rootPath: string | null }) =>
      ipc.createLibrary(args.name, args.sourceType, args.rootPath),
    onSuccess: () => qc.invalidateQueries({ queryKey: qk.libraries }),
  });
}

export function useStartScan() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (libraryId: string) => ipc.startLibraryScan(libraryId),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: qk.jobs });
      toast.info("Scan queued");
    },
    onError: (e: Error) => toast.danger("Could not start scan", e.message),
  });
}

export function useDeleteLibrary() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => ipc.deleteLibrary(id),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: qk.libraries });
      qc.invalidateQueries({ queryKey: qk.styles });
    },
    onError: (e: Error) => toast.danger("Could not remove library", e.message),
  });
}
