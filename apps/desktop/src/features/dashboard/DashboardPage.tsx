import { useState } from "react";
import { Link } from "react-router-dom";
import { Badge, Button, Card, EmptyState, InlineError } from "@mimic/ui";
import {
  CHANNEL_LABELS,
  type Channel,
  type Dashboard,
  type DashboardThread,
  type Draft,
  describeFeed,
} from "@mimic/contracts";
import { PageHeader } from "@/components/PageHeader";
import { useDashboard, useStartAssistDrafts } from "@/hooks/useDashboard";
import { useGenerateDraft, useResolveDraft } from "@/hooks/useCompose";
import { useSettings } from "@/hooks/useSystem";
import { ComposePanel } from "@/features/compose/ComposePanel";
import { formatRelative } from "@/lib/format";
import { toast } from "@/state/toast";
import { ipc } from "@/lib/ipc";

/**
 * The home screen: what is waiting on you, and what Mimic has written for it.
 *
 * Two claims this screen must never make. It is not an inbox — every message
 * on it arrived through an import, and the header says when the last one was.
 * And a draft on it is a draft: approving records what you sent and teaches
 * Mimic from it. Nothing here sends anything.
 */
export function DashboardPage() {
  const dashboard = useDashboard();
  const assist = useStartAssistDrafts();
  const settings = useSettings();
  const data = dashboard.data;

  if (dashboard.isError) {
    return <InlineError>{(dashboard.error as Error).message}</InlineError>;
  }

  if (data && data.messages === 0) {
    return (
      <div className="stack gap-3">
        <PageHeader title="Mimic" subtitle="Nothing has been imported yet." />
        <EmptyState
          title="No messages yet"
          body="Mimic reads exports you point it at — a .mbox from Gmail Takeout or Thunderbird, or its own JSON format. It is not connected to a mailbox and nothing arrives on its own."
          primary={
            <Link to="/sources">
              <Button variant="primary">Add a source</Button>
            </Link>
          }
        />
        <ComposePanel title="Write something anyway" />
      </div>
    );
  }

  return (
    <div className="stack gap-3">
      <PageHeader title="Mimic" subtitle={data ? describeFeed(data) : "Loading what is waiting…"} />

      {data ? <Counts data={data} /> : null}

      <Card
        title="Waiting on you"
        actions={
          data?.autoDraft ? (
            <Button
              size="sm"
              variant="ghost"
              disabled={assist.isPending}
              onClick={() => assist.mutate()}
            >
              {assist.isPending ? "Queuing…" : "Prepare replies now"}
            </Button>
          ) : null
        }
      >
        {data?.autoDraft ? (
          <p className="muted small">
            Mimic prepares replies for these threads by itself after each import. Turn it off in
            Settings if you would rather ask each time.
          </p>
        ) : (
          <p className="muted small">
            Mimic drafts only when you ask. You can let it prepare replies in advance in Settings —
            that sends the incoming message to{" "}
            {settings.data?.["generation.provider"] === "anthropic"
              ? "a hosted model"
              : "your model"}{" "}
            without you asking each time.
          </p>
        )}
        {data && data.awaiting.length === 0 ? (
          <p className="neutral">
            Nothing in what has been imported ends with someone else&rsquo;s message.
          </p>
        ) : null}
        <div className="stack gap-2">
          {(data?.awaiting ?? []).map((t) => (
            <ThreadRow key={t.conversationId} thread={t} />
          ))}
        </div>
      </Card>

      <ComposePanel title="Write something new" />
    </div>
  );
}

function Counts({ data }: { data: Dashboard }) {
  return (
    <div className="row gap-3 wrap muted small">
      <span>{data.messages.toLocaleString()} messages imported</span>
      <span>{data.ownMessages.toLocaleString()} written by you</span>
      <span>{data.conversations.toLocaleString()} conversations</span>
      <span>{data.people.toLocaleString()} people</span>
      <span>Last import {formatRelative(data.lastImportAt)}</span>
    </div>
  );
}

/**
 * One waiting thread. The message is shown in full rather than truncated to a
 * teaser: deciding whether a prepared reply is right is impossible without it.
 */
function ThreadRow({ thread }: { thread: DashboardThread }) {
  const generate = useGenerateDraft();
  const [draft, setDraft] = useState<Draft | null>(thread.draft);
  const who = thread.participant?.displayName ?? "Someone Mimic could not identify";
  const channel = CHANNEL_LABELS[thread.channel as Channel] ?? thread.channel;

  async function writeDraft() {
    const result = await generate.mutateAsync({
      participantId: thread.participant?.id ?? null,
      conversationId: thread.conversationId,
      channel: thread.channel,
      incomingMessage: thread.lastMessage,
      intent: null,
    });
    setDraft(result);
  }

  return (
    <div className="thread">
      <div className="row gap-2 between">
        <div>
          <strong>{who}</strong>{" "}
          <span className="muted small">
            {channel}
            {thread.subject ? ` · ${thread.subject}` : ""} · {formatRelative(thread.lastMessageAt)}
          </span>
        </div>
        <div className="row gap-2">
          {thread.isGroup ? <Badge tone="neutral">Group</Badge> : null}
          {thread.participant ? (
            <Badge tone={thread.hasRelationshipProfile ? "success" : "neutral"}>
              {thread.hasRelationshipProfile ? "How you write to them" : "How you write in general"}
            </Badge>
          ) : (
            <Badge tone="warning">Unattributed</Badge>
          )}
        </div>
      </div>

      <blockquote className="thread__message">{thread.lastMessage}</blockquote>

      {draft ? (
        <DraftReview draft={draft} onDone={() => setDraft(null)} />
      ) : (
        <div className="row gap-2">
          <Button size="sm" variant="primary" onClick={writeDraft} disabled={generate.isPending}>
            {generate.isPending ? "Writing…" : "Draft a reply"}
          </Button>
          <Link to={`/people`} className="muted small">
            {thread.participant ? `Open ${who} in People` : "Set who this is in People"}
          </Link>
        </div>
      )}
      {generate.isError ? <InlineError>{(generate.error as Error).message}</InlineError> : null}
    </div>
  );
}

/**
 * Approve, modify, or reject. "Approve" copies the text and records that you
 * sent it as written; "Modify" records what you changed, which is the only
 * thing that teaches Mimic anything. Neither sends: Mimic has no send path,
 * deliberately, and adding one is a decision to make on purpose rather than by
 * accident.
 */
export function DraftReview({ draft, onDone }: { draft: Draft; onDone: () => void }) {
  const resolve = useResolveDraft();
  const [text, setText] = useState(draft.generatedText);
  const [editing, setEditing] = useState(false);
  const edited = text !== draft.generatedText;

  async function approve() {
    await navigator.clipboard.writeText(text).catch(() => undefined);
    await resolve.mutateAsync({
      draftId: draft.id,
      outcome: edited ? "sent_edited" : "sent_unedited",
      finalText: text,
    });
    toast.success(
      edited ? "Copied, and Mimic noted what you changed" : "Copied",
      "Paste it wherever you are replying — Mimic does not send.",
    );
    onDone();
  }

  async function reject() {
    await resolve.mutateAsync({ draftId: draft.id, outcome: "discarded", finalText: null });
    onDone();
  }

  return (
    <div className="draft-review">
      {editing ? (
        <textarea rows={5} value={text} onChange={(e) => setText(e.target.value)} autoFocus />
      ) : (
        <p className="draft-review__text">{text}</p>
      )}
      <div className="row gap-2">
        <Button size="sm" variant="primary" onClick={approve} disabled={resolve.isPending}>
          {edited ? "Copy my version and record it" : "Approve and copy"}
        </Button>
        <Button size="sm" variant="ghost" onClick={() => setEditing((v) => !v)}>
          {editing ? "Stop editing" : "Modify"}
        </Button>
        <Button size="sm" variant="ghost" onClick={reject} disabled={resolve.isPending}>
          Reject
        </Button>
        <Button
          size="sm"
          variant="ghost"
          onClick={async () => {
            const note = window.prompt("What should Mimic do differently next time?");
            if (!note?.trim()) return;
            await ipc.addDraftPreference(draft.id, note.trim());
            toast.success("Noted", "A stated preference outweighs anything inferred from an edit.");
          }}
        >
          Tell Mimic why
        </Button>
      </div>
      <p className="muted small">
        {draft.intent
          ? "Written from what you said you wanted to say."
          : "Prepared from the incoming message and how you write — you never stated an intent, so check it says what you mean."}{" "}
        Approving copies it and records what you sent; Mimic never sends anything itself.
      </p>
    </div>
  );
}
