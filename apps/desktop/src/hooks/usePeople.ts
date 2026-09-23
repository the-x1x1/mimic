import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { describeClaimed, type AddressAdded, type AddressOwner } from "@mimic/contracts";
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

/**
 * Add an address of the user's. When someone was folded back into the user,
 * who wrote what changed everywhere — people, threads, drafts, how the user
 * writes — so everything is read again, and the user is told how many
 * messages moved. A failure is shown where the address was typed, not in a
 * toast as well.
 */
export function useAddIdentifier() {
  const onAdded = useOnAddressAdded();
  return useMutation({
    mutationFn: ({
      kind,
      value,
      confirmedOwner = null,
    }: {
      kind: string;
      value: string;
      confirmedOwner?: AddressOwner | null;
    }) => ipc.addUserIdentifier(kind, value, confirmedOwner),
    onSuccess: onAdded,
  });
}

/** Settle a held address by folding its holder in, as adding it would have. */
export function useClaimHeldAddress() {
  const onAdded = useOnAddressAdded();
  return useMutation({
    mutationFn: ({ identifierId, owner }: { identifierId: string; owner: AddressOwner }) =>
      ipc.claimHeldAddress(identifierId, owner),
    onSuccess: onAdded,
  });
}

/** The person under one of the user's addresses is not them. */
export function useKeepApart() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (participantId: string) => ipc.keepPersonApart(participantId),
    onSuccess: () => qc.invalidateQueries({ queryKey: qk.heldAddresses }),
  });
}

function useOnAddressAdded() {
  const qc = useQueryClient();
  return (added: AddressAdded) => {
    if (added.claimed.people > 0) {
      void qc.invalidateQueries();
    } else {
      void qc.invalidateQueries({ queryKey: qk.identity });
      void qc.invalidateQueries({ queryKey: qk.onboarding });
    }
    const said = describeClaimed(added.claimed);
    if (said) toast.info("That address is yours", said);
  };
}

/**
 * Refused because what the user was shown is not what is there now — a check
 * filed more mail under the person, or something was written about them.
 */
export function isStaleConfirmation(e: unknown): boolean {
  return (e as { code?: unknown } | null)?.code === "confirm";
}

/** What adding an address would do, asked before adding it. Changes nothing. */
export function usePreviewAddress() {
  return useMutation({
    mutationFn: ({ kind, value }: { kind: string; value: string }) =>
      ipc.previewUserAddress(kind, value),
  });
}

/** The user's addresses that mail already read is still filed under someone else by. */
export function useHeldAddresses(enabled = true) {
  return useQuery({ queryKey: qk.heldAddresses, queryFn: ipc.heldUserAddresses, enabled });
}

export function useRemoveIdentifier() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ipc.removeUserIdentifier,
    onSuccess: () => qc.invalidateQueries({ queryKey: qk.identity }),
  });
}

/**
 * People — not the senders that have only ever sent automated mail, unless
 * asked for. Every place a person is picked uses this list.
 */
export function usePeople(showAutomated = false, enabled = true) {
  return useQuery({
    queryKey: [...qk.people, showAutomated],
    // The senders' query asks for no people: the page already has them.
    queryFn: () => (showAutomated ? ipc.people(0, 200) : ipc.people(200, null)),
    enabled,
  });
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
