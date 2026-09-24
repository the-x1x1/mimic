import { useState } from "react";
import * as Dialog from "@radix-ui/react-dialog";
import { useInfiniteQuery } from "@tanstack/react-query";
import { Button, InlineError } from "@mimic/ui";
import {
  CHANNEL_LABELS,
  canPutOnList,
  describeStanding,
  type Channel,
  type PersonConversation,
} from "@mimic/contracts";
import { ipc } from "@/lib/ipc";
import { qk } from "@/app/queryClient";
import { useMarkThread } from "@/hooks/useDashboard";
import { formatRelative } from "@/lib/format";
import { ThreadMessages } from "@/features/replies/ThreadMessages";

const PAGE = 20;

/** Said when a conversation being read was deleted meanwhile. */
export const CONVERSATION_GONE = "This conversation isn't here any more.";

function isGone(e: unknown): boolean {
  return (e as { code?: unknown } | null)?.code === "not_found";
}

/**
 * Every conversation someone is in, not only the one waiting on the home
 * screen: most recently active first, each with where it stands by the same
 * reading the home screen makes, and each readable in full. One left off the
 * list — it looks automated, it has gone quiet, or the user said it needs no
 * reply — can be put back on it from here, which is how a reply gets drafted
 * for it.
 */
export function PersonConversations({
  participantId,
  name,
  onClose,
}: {
  participantId: string;
  name: string;
  onClose: () => void;
}) {
  const pages = useInfiniteQuery({
    queryKey: qk.personConversations(participantId),
    queryFn: ({ pageParam }) => ipc.personConversations(participantId, pageParam, PAGE),
    initialPageParam: null as { at: string; id: string } | null,
    // Read on from the last conversation shown.
    getNextPageParam: (last) => {
      const end = last.conversations.at(-1);
      return last.more > 0 && end
        ? { at: end.lastMessageAt ?? "", id: end.conversationId }
        : undefined;
    },
  });
  const read = pages.data?.pages ?? [];
  const conversations = read.flatMap((p) => p.conversations);
  const withinDays = read[0]?.waitingWithinDays ?? null;
  const more = read.at(-1)?.more ?? 0;

  return (
    <Dialog.Root open onOpenChange={(o) => !o && onClose()}>
      <Dialog.Portal>
        <Dialog.Overlay className="dialog__overlay" />
        <Dialog.Content className="dialog dialog--wide">
          <Dialog.Title className="dialog__title">You and {name}</Dialog.Title>
          <Dialog.Description className="neutral">
            Every conversation I&rsquo;ve read that {name} is in, the most recent first, and whether
            it&rsquo;s on your list.
          </Dialog.Description>
          {pages.isError ? <InlineError>{pages.error.message}</InlineError> : null}
          {pages.isPending ? <p className="muted small">Looking&hellip;</p> : null}
          {pages.isSuccess && conversations.length === 0 ? (
            <p className="muted">I haven&rsquo;t read a conversation with {name} in it.</p>
          ) : null}
          {conversations.length > 0 ? (
            <ol className="stack gap-2">
              {conversations.map((c) => (
                <ConversationItem key={c.conversationId} c={c} withinDays={withinDays} />
              ))}
            </ol>
          ) : null}
          {more > 0 ? (
            <button
              type="button"
              className="linkish"
              disabled={pages.isFetchingNextPage}
              onClick={() => void pages.fetchNextPage()}
            >
              {pages.isFetchingNextPage
                ? "Looking further back…"
                : `Show ${more.toLocaleString()} more ${more === 1 ? "conversation" : "conversations"}`}
            </button>
          ) : null}
          <div className="row gap-2 dialog__actions">
            <Button variant="ghost" onClick={onClose}>
              Close
            </Button>
          </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}

function ConversationItem({ c, withinDays }: { c: PersonConversation; withinDays: number | null }) {
  const [open, setOpen] = useState(false);
  const mark = useMarkThread();
  const subject = c.subject?.trim() || "No subject";
  const deciding = c.decidingMessageId;
  const region = `conversation-${c.conversationId}`;
  const facts = [
    CHANNEL_LABELS[c.channel as Channel] ?? c.channel,
    c.messageCount === 1 ? "1 message" : `${c.messageCount.toLocaleString()} messages`,
    c.isGroup ? "with others too" : null,
    c.lastMessageAt ? formatRelative(c.lastMessageAt) : null,
  ].filter((f): f is string => f !== null);
  return (
    <li className="stack gap-1">
      <div className="row gap-2">
        <strong>{subject}</strong>
        <span className="muted small">{facts.join(" · ")}</span>
      </div>
      <p className="muted small">
        {describeStanding(c, withinDays)}{" "}
        {canPutOnList(c) && deciding ? (
          <button
            type="button"
            className="linkish"
            aria-label={`Put it on my list: ${subject}`}
            disabled={mark.isPending}
            onClick={() =>
              mark.mutate({
                conversationId: c.conversationId,
                messageId: deciding,
                mark: "needs_reply",
              })
            }
          >
            Put it on my list
          </button>
        ) : null}
      </p>
      <div>
        <button
          type="button"
          className="linkish"
          aria-expanded={open}
          aria-controls={open ? region : undefined}
          aria-label={`${open ? "Hide it" : "Read it"}: ${subject}`}
          onClick={() => setOpen((o) => !o)}
        >
          {open ? "Hide it" : "Read it"}
        </button>
      </div>
      {open ? (
        <section id={region} aria-label={subject}>
          <WholeConversation conversationId={c.conversationId} />
        </section>
      ) : null}
    </li>
  );
}

/**
 * A conversation read from its end, a page at a time toward its start, the
 * earliest on top — the way the rest of a waiting thread is read on the home
 * screen, from the last message rather than the waiting one.
 */
export function WholeConversation({ conversationId }: { conversationId: string }) {
  const pages = useInfiniteQuery({
    queryKey: qk.conversationEnd(conversationId),
    queryFn: ({ pageParam }) =>
      pageParam === null
        ? ipc.conversationEnd(conversationId, PAGE)
        : ipc.conversationPage(conversationId, pageParam, "earlier", PAGE),
    initialPageParam: null as string | null,
    // Read further back from the earliest message shown.
    getNextPageParam: (last) => {
      const first = last.messages[0];
      return last.more > 0 && first ? first.id : undefined;
    },
  });
  if (pages.isError && isGone(pages.error)) {
    return <p className="muted small">{CONVERSATION_GONE}</p>;
  }
  const read = pages.data?.pages ?? [];
  // Each page is further back than the one before it, so the last read goes on top.
  const messages = [...read].reverse().flatMap((p) => p.messages);
  const more = read.at(-1)?.more ?? 0;
  return (
    <div className="thread__rest">
      {pages.isError ? <InlineError>{pages.error.message}</InlineError> : null}
      {pages.isPending ? <p className="muted small">Reading it back…</p> : null}
      {more > 0 ? (
        <button
          type="button"
          className="linkish"
          disabled={pages.isFetchingNextPage}
          onClick={() => void pages.fetchNextPage()}
        >
          {pages.isFetchingNextPage
            ? "Reading further back…"
            : `Show ${more.toLocaleString()} earlier ${more === 1 ? "message" : "messages"}`}
        </button>
      ) : null}
      {messages.length > 0 ? <ThreadMessages messages={messages} /> : null}
    </div>
  );
}
