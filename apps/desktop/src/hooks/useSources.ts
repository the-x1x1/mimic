import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { describeImport, ImportSummary } from "@mimic/contracts";
import { ipc } from "@/lib/ipc";
import { qk } from "@/app/queryClient";
import { toast } from "@/state/toast";

export function useConnectors() {
  return useQuery({ queryKey: qk.connectors, queryFn: ipc.connectors, staleTime: Infinity });
}

export function useSources() {
  return useQuery({ queryKey: qk.sources, queryFn: ipc.sources });
}

export function useCreateSource() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ipc.createSource,
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: qk.sources });
      qc.invalidateQueries({ queryKey: qk.onboarding });
    },
    onError: (e: Error) => toast.danger("Could not add that source", e.message),
  });
}

export function useStartImport() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ipc.startSourceImport,
    onSuccess: () => qc.invalidateQueries({ queryKey: qk.jobs }),
    onError: (e: Error) => toast.danger("Could not start the import", e.message),
  });
}

export function useDeleteSource() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ipc.deleteSource,
    onSuccess: (report) => {
      for (const key of [qk.sources, qk.people, qk.voice, qk.system, qk.onboarding, qk.providers]) {
        qc.invalidateQueries({ queryKey: key });
      }
      toast.info(
        "Source removed",
        `${report.messages.toLocaleString()} messages and ${report.participants} people went with it.`,
      );
    },
    onError: (e: Error) => toast.danger("Could not remove that source", e.message),
  });
}

/**
 * A connected mailbox's password, given again. Resolves once the login worked
 * and the password is saved; a check follows by itself.
 */
export function useSetMailboxPassword() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ sourceId, password }: { sourceId: string; password: string }) =>
      ipc.setMailboxPassword(sourceId, password),
    onSuccess: () => {
      for (const key of [qk.sources, qk.jobs, qk.dashboard, qk.providers]) {
        void qc.invalidateQueries({ queryKey: key });
      }
    },
  });
}

/** Whether this copy of Mimic can sign in to a mailbox with Microsoft. It can't change while it runs. */
export function useMailSignIn() {
  return useQuery({
    queryKey: qk.mailSignIn,
    queryFn: ipc.mailSignInAvailable,
    staleTime: Infinity,
  });
}

/**
 * Sign a connected Microsoft mailbox in again. Resolves once the browser has
 * come back and the new sign-in opened the mailbox.
 */
export function useSignInMailboxAgain() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (sourceId: string) => ipc.signInMailboxAgain(sourceId),
    onSuccess: () => {
      for (const key of [qk.sources, qk.jobs, qk.dashboard, qk.providers]) {
        void qc.invalidateQueries({ queryKey: key });
      }
    },
  });
}

/** Turn a finished import job's result into the sentence the tray shows. */
export function summarizeImport(result: unknown): string | null {
  const parsed = ImportSummary.safeParse(result);
  return parsed.success ? describeImport(parsed.data) : null;
}
