import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ipc } from "@/lib/ipc";
import { qk } from "@/app/queryClient";
import { toast } from "@/state/toast";

export function useIdentity() {
  return useQuery({ queryKey: qk.identity, queryFn: ipc.userIdentity });
}

export function useSetIdentity() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ipc.setUserIdentity,
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: qk.identity });
      qc.invalidateQueries({ queryKey: qk.onboarding });
    },
    onError: (e: Error) => toast.danger("Could not save that", e.message),
  });
}

export function useAddIdentifier() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ kind, value }: { kind: string; value: string }) =>
      ipc.addUserIdentifier(kind, value),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: qk.identity });
      qc.invalidateQueries({ queryKey: qk.onboarding });
    },
    onError: (e: Error) => toast.danger("Could not add that address", e.message),
  });
}

export function useRemoveIdentifier() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ipc.removeUserIdentifier,
    onSuccess: () => qc.invalidateQueries({ queryKey: qk.identity }),
  });
}

export function usePeople() {
  return useQuery({ queryKey: qk.people, queryFn: () => ipc.people() });
}

export function useSetRelationship() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ id, relationship }: { id: string; relationship: string | null }) =>
      ipc.setPersonRelationship(id, relationship),
    onSuccess: () => qc.invalidateQueries({ queryKey: qk.people }),
    onError: (e: Error) => toast.danger("Could not save that", e.message),
  });
}

export function useDeletionPreview(participantId: string | null) {
  return useQuery({
    queryKey: ["deletionPreview", participantId],
    queryFn: () => ipc.previewPersonDeletion(participantId as string),
    enabled: Boolean(participantId),
  });
}

export function useDeletePerson() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ipc.deletePerson,
    onSuccess: (report) => {
      for (const key of [qk.people, qk.voice, qk.system, qk.drafts]) {
        qc.invalidateQueries({ queryKey: key });
      }
      toast.info(
        "Deleted",
        `${report.messages.toLocaleString()} messages removed. Your voice profile needs recomputing.`,
      );
    },
    onError: (e: Error) => toast.danger("Could not delete", e.message),
  });
}
