import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { describeImport, ImportSummary, type SignInSetup } from "@mimic/contracts";
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

/** Connect a Slack workspace; a check follows by itself (0.10.0-alpha.64). */
export function useConnectSlack() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (token: string) => ipc.connectSlack(token),
    onSuccess: () => {
      for (const key of [qk.sources, qk.jobs, qk.onboarding, qk.dashboard, qk.settings]) {
        void qc.invalidateQueries({ queryKey: key });
      }
    },
  });
}

/** Sign in for Teams and connect it; a check follows by itself (0.10.0-alpha.67). */
export function useConnectTeams() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: () => ipc.connectTeams(),
    onSuccess: () => {
      for (const key of [qk.sources, qk.jobs, qk.onboarding, qk.dashboard, qk.settings]) {
        void qc.invalidateQueries({ queryKey: key });
      }
    },
  });
}

/** Sign a connected Teams account in again; a check follows by itself. */
export function useSignInTeamsAgain() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (sourceId: string) => ipc.signInTeamsAgain(sourceId),
    onSuccess: () => {
      for (const key of [qk.sources, qk.jobs, qk.dashboard]) {
        void qc.invalidateQueries({ queryKey: key });
      }
    },
  });
}

/** Sign in for Google Chat and connect it; a check follows by itself (0.10.0-alpha.79). */
export function useConnectGoogleChat() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: () => ipc.connectGoogleChat(),
    onSuccess: () => {
      for (const key of [qk.sources, qk.jobs, qk.onboarding, qk.dashboard, qk.settings]) {
        void qc.invalidateQueries({ queryKey: key });
      }
    },
  });
}

/** Sign a connected Google Chat account in again; a check follows by itself. */
export function useSignInGoogleChatAgain() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (sourceId: string) => ipc.signInGoogleChatAgain(sourceId),
    onSuccess: () => {
      for (const key of [qk.sources, qk.jobs, qk.dashboard]) {
        void qc.invalidateQueries({ queryKey: key });
      }
    },
  });
}

/** A connected workspace's new token; a check follows by itself. */
export function useSetSlackToken() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ sourceId, token }: { sourceId: string; token: string }) =>
      ipc.setSlackToken(sourceId, token),
    onSuccess: () => {
      for (const key of [qk.sources, qk.jobs, qk.dashboard]) {
        void qc.invalidateQueries({ queryKey: key });
      }
    },
  });
}

/**
 * Which providers this copy of Mimic can sign in to a mailbox with. It
 * changes only when the user saves or forgets their own registration
 * (`useSaveSignInSetup`), which refreshes it.
 */
export function useMailSignIn() {
  return useQuery({
    queryKey: qk.mailSignIn,
    queryFn: ipc.mailSignInAvailable,
    staleTime: Infinity,
  });
}

/** What each provider signs mailboxes in as (0.10.0-alpha.59). */
export function useSignInSetup() {
  return useQuery({ queryKey: qk.signInSetup, queryFn: ipc.signInSetup });
}

/** Keep, or forget, the user's own registration with a provider. */
export function useSaveSignInSetup() {
  const qc = useQueryClient();
  const settled = (data: SignInSetup) => {
    qc.setQueryData(qk.signInSetup, data);
    qc.invalidateQueries({ queryKey: qk.mailSignIn });
  };
  const save = useMutation({
    mutationFn: (v: {
      with: "microsoft" | "google";
      clientId: string;
      clientSecret: string | null;
    }) => ipc.saveSignInSetup(v.with, v.clientId, v.clientSecret),
    onSuccess: settled,
  });
  const forget = useMutation({
    mutationFn: (withProvider: "microsoft" | "google") => ipc.forgetSignInSetup(withProvider),
    onSuccess: settled,
  });
  return { save, forget };
}

/**
 * Who handles mail for a domain no provider is known for, asked once per
 * domain while Mimic runs; nothing is asked while `domain` is null.
 */
export function useMailExchangers(domain: string | null) {
  return useQuery({
    queryKey: qk.mailExchangers(domain ?? ""),
    queryFn: () => ipc.mailExchangers(domain ?? ""),
    enabled: domain !== null,
    staleTime: Infinity,
  });
}

/**
 * Sign a connected Microsoft or Google mailbox in again. Resolves once the browser has
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

/**
 * The folders a mailbox may be read in as well (0.10.0-alpha.73). Asking
 * signs in to the mailbox, so only while `shown`, and not again on focus.
 */
export function useMailboxFolders(sourceId: string, shown: boolean) {
  return useQuery({
    queryKey: qk.mailboxFolders(sourceId),
    queryFn: () => ipc.mailboxFolders(sourceId),
    enabled: shown,
    staleTime: 60_000,
    refetchOnWindowFocus: false,
    retry: false,
  });
}

export function useSetMailboxFolders(sourceId: string) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (folders: string[]) => ipc.setMailboxFolders(sourceId, folders),
    onSuccess: (data) => {
      qc.setQueryData(qk.mailboxFolders(sourceId), data);
      // A check has started.
      qc.invalidateQueries({ queryKey: qk.sources });
      qc.invalidateQueries({ queryKey: qk.jobs });
    },
  });
}

/** Whether the Telegram app registration is kept (0.10.0-alpha.74). */
export function useTelegramApp(enabled: boolean) {
  return useQuery({ queryKey: qk.telegramApp, queryFn: ipc.telegramApp, enabled });
}

/**
 * The steps of a Telegram sign-in: ask for a code, give it, give the
 * password. When one ends in `done`, a check has started.
 */
export function useTelegramSignIn() {
  const qc = useQueryClient();
  const settled = (step: { step: string }) => {
    if (step.step !== "done") return;
    for (const key of [
      qk.sources,
      qk.jobs,
      qk.onboarding,
      qk.dashboard,
      qk.settings,
      qk.telegramApp,
    ]) {
      void qc.invalidateQueries({ queryKey: key });
    }
  };
  const start = useMutation({
    mutationFn: (v: { phone: string; apiId: string | null; apiHash: string | null }) =>
      ipc.startTelegramSignIn(v.phone, v.apiId, v.apiHash),
    onSuccess: () => void qc.invalidateQueries({ queryKey: qk.telegramApp }),
  });
  const code = useMutation({
    mutationFn: (code: string) => ipc.telegramSignInCode(code),
    onSuccess: settled,
  });
  const password = useMutation({
    mutationFn: (password: string) => ipc.telegramSignInPassword(password),
    onSuccess: settled,
  });
  return { start, code, password };
}
