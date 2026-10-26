import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import type { AutoReplyRules, SendRequest, SmtpAccount } from "@mimic/contracts";
import { ipc } from "@/lib/ipc";
import { qk } from "@/app/queryClient";

/** Who a reply to this draft would go to, and through what, before anything goes. */
export function useReplyEnvelope(draftId: string) {
  return useQuery({ queryKey: qk.envelope(draftId), queryFn: () => ipc.replyEnvelope(draftId) });
}

export function useSendReply() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (request: SendRequest) => ipc.sendReply(request),
    // Settled, not only succeeded: a reply that failed is in the list of
    // what was sent too, saying why, and a draft decided elsewhere is
    // refused and should leave the screen.
    onSettled: () => {
      qc.invalidateQueries({ queryKey: qk.sent });
      qc.invalidateQueries({ queryKey: qk.drafts });
      qc.invalidateQueries({ queryKey: qk.draftOutcomes });
      qc.invalidateQueries({ queryKey: qk.dashboard });
      // A sent draft is evidence for the learning loop.
      qc.invalidateQueries({ queryKey: qk.learning });
    },
  });
}

/** What Mimic sent or holds to send, newest first. */
export function useSent(limit = 25) {
  return useQuery({ queryKey: qk.sent, queryFn: () => ipc.sent(limit) });
}

/** A reply that may have gone didn't: the user looked. */
export function useReplyDidNotGo() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (outboxId: string) => ipc.replyDidNotGo(outboxId),
    onSettled: () => {
      qc.invalidateQueries({ queryKey: qk.sent });
      // Its draft can be sent again.
      qc.invalidateQueries({ queryKey: qk.dashboard });
    },
  });
}

export function useMailboxSending(sourceId: string) {
  return useQuery({
    queryKey: qk.mailboxSending(sourceId),
    queryFn: () => ipc.mailboxSending(sourceId),
  });
}

export function useSetMailboxSending(sourceId: string) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (sending: SmtpAccount | null) =>
      sending ? ipc.setMailboxSending(sourceId, sending) : ipc.resetMailboxSending(sourceId),
    onSuccess: (data) => {
      qc.setQueryData(qk.mailboxSending(sourceId), data);
      // Which drafts can be sent, and through what, follows.
      qc.invalidateQueries({ queryKey: qk.dashboard });
    },
  });
}

/** The rules for replies sent by themselves, whether they run, and the last day's count. */
export function useAutoReply() {
  return useQuery({ queryKey: qk.autoReply, queryFn: ipc.autoReply });
}

export function useSetAutoReply() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (rules: AutoReplyRules) => ipc.setAutoReply(rules),
    onSuccess: (data) => {
      qc.setQueryData(qk.autoReply, data);
      // Turning it on turns on preparing replies in advance.
      qc.invalidateQueries({ queryKey: qk.settings });
      qc.invalidateQueries({ queryKey: qk.dashboard });
      qc.invalidateQueries({ queryKey: qk.sent });
    },
  });
}

/** Stop a reply held to go by itself, or let it go now. */
export function useHeldReply() {
  const qc = useQueryClient();
  const settle = () => {
    qc.invalidateQueries({ queryKey: qk.dashboard });
    qc.invalidateQueries({ queryKey: qk.sent });
    qc.invalidateQueries({ queryKey: qk.drafts });
  };
  const stop = useMutation({
    mutationFn: (outboxId: string) => ipc.stopHeldReply(outboxId),
    onSettled: settle,
  });
  const now = useMutation({
    mutationFn: (outboxId: string) => ipc.sendHeldReplyNow(outboxId),
    onSettled: settle,
  });
  return { stop, now };
}
