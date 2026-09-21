import { useState } from "react";
import { Link } from "react-router-dom";
import { Button, InlineError } from "@mimic/ui";
import {
  type Dashboard,
  type DashboardThread,
  type Draft,
  describeWaiting,
} from "@mimic/contracts";
import { useDashboard, useStartAssistDrafts } from "@/hooks/useDashboard";
import { useGenerateDraft, useResolveDraft } from "@/hooks/useCompose";
import { formatRelative } from "@/lib/format";
import { toast } from "@/state/toast";
import { ipc } from "@/lib/ipc";

/**
 * The one screen: who is waiting, what they said, and what Mimic would say
 * back.
 *
 * Two claims it must never make. It is not an inbox — every message on it
 * arrived through an import, and the first sentence says so before it says
 * anything else. And a draft is a draft: using one copies it and records what
 * you sent. Nothing here sends.
 */
export function RepliesPage() {
  const dashboard = useDashboard();
  const data = dashboard.data;

  if (dashboard.isError) {
    return <InlineError>{(dashboard.error as Error).message}</InlineError>;
  }

  if (!data) {
    return <p className="muted">Looking at what&rsquo;s waiting&hellip;</p>;
  }

  return (
    <div className="replies">
      <div className="replies__head">
        <h1 className="replies__title">{describeWaiting(data)}</h1>
        {data.awaitingTotal > 0 ? (
          <p className="replies__lede">
            I&rsquo;ve written a reply for each one, in your words. Read it, change anything you
            like, then send it yourself &mdash; I never send anything.
          </p>
        ) : null}
      </div>

      {data.messages === 0 ? <NothingYet /> : null}
      {data.messages > 0 && data.awaiting.length === 0 ? <CaughtUp /> : null}

      <div className="replies__list">
        {data.awaiting.map((t) => (
          <Thread key={t.conversationId} thread={t} />
        ))}
      </div>

      {data.awaiting.length > 0 ? <Footer data={data} /> : null}
    </div>
  );
}

function NothingYet() {
  return (
    <section className="note">
      <h2 className="note__title">There&rsquo;s nothing here yet.</h2>
      <p className="note__body">
        I haven&rsquo;t read any of your mail, so I don&rsquo;t know how you write or who you write
        to. Point me at a copy of your old mail and this fills up. Nothing leaves this computer, and
        nothing arrives on its own &mdash; I only read what you hand me.
      </p>
      <div className="note__actions">
        <Link to="/sources">
          <Button variant="primary">Add your mail</Button>
        </Link>
        <Link to="/settings" className="muted">
          Finish setting up
        </Link>
      </div>
    </section>
  );
}

function CaughtUp() {
  return (
    <section className="note">
      <h2 className="note__title">You&rsquo;re all caught up.</h2>
      <p className="note__body">
        Nothing in the mail I&rsquo;ve read ends with someone else waiting on you. Bring in newer
        mail and I&rsquo;ll have replies ready for anything new.
      </p>
      <div className="note__actions">
        <Link to="/sources">
          <Button>Bring in new mail</Button>
        </Link>
        <Link to="/write" className="muted">
          Write something new
        </Link>
      </div>
    </section>
  );
}

/** What Mimic is doing in the background, and how to change it. Quiet, last. */
function Footer({ data }: { data: Dashboard }) {
  const assist = useStartAssistDrafts();
  return (
    <div className="replies__foot">
      <p className="muted small">
        {data.autoDraft
          ? "I write these by myself after each import. If you would rather I asked first, turn that off in Settings."
          : "I only write when you ask. I can have them ready in advance instead — that is in Settings."}{" "}
        I have read {data.messages.toLocaleString()} of your emails,{" "}
        {data.ownMessages.toLocaleString()} of them written by you. Last one came in{" "}
        {formatRelative(data.lastImportAt)}.
      </p>
      <div className="row gap-3 wrap">
        {data.autoDraft ? (
          <Button size="sm" disabled={assist.isPending} onClick={() => assist.mutate()}>
            {assist.isPending ? "Working…" : "Write the rest now"}
          </Button>
        ) : null}
        <Link to="/write" className="muted small">
          Write something new
        </Link>
      </div>
    </div>
  );
}

/**
 * One person waiting. The message is shown in full rather than as a teaser:
 * deciding whether a prepared reply is right is impossible without it.
 */
function Thread({ thread }: { thread: DashboardThread }) {
  const generate = useGenerateDraft();
  const [draft, setDraft] = useState<Draft | null>(thread.draft);
  const [intent, setIntent] = useState("");
  const who = thread.participant?.displayName ?? "Someone I couldn't put a name to";
  const address = thread.participant?.identifiers?.[0]?.value ?? null;
  // "She wrote" needs a gender nobody told us. "They wrote" is wrong for one
  // named person to some readers and right to others, so the label names the
  // person instead and sidesteps a guess.
  const saidLabel = thread.isGroup ? "They wrote" : `${who.split(" ")[0]} wrote`;

  async function writeDraft() {
    const result = await generate.mutateAsync({
      participantId: thread.participant?.id ?? null,
      conversationId: thread.conversationId,
      channel: thread.channel,
      incomingMessage: thread.lastMessage,
      intent: intent.trim() || null,
    });
    setDraft(result);
  }

  return (
    <article className="thread">
      <header className="thread__head">
        <div className="thread__who">
          <span className="thread__name">{who}</span>
          {address ? <span className="thread__address">{address}</span> : null}
        </div>
        <span className="thread__when">{formatRelative(thread.lastMessageAt)}</span>
      </header>

      {thread.subject ? <p className="thread__subject">{thread.subject}</p> : null}

      <div className="thread__part">
        <div className="letter-label">{saidLabel}</div>
        <p className="letter letter--theirs">{thread.lastMessage}</p>
      </div>

      {draft ? (
        <DraftReview draft={draft} onDone={() => setDraft(null)} />
      ) : (
        <div className="thread__part thread__part--ruled">
          <div className="letter-label">I haven&rsquo;t written this one yet</div>
          <p className="muted">
            {thread.participant && !thread.hasRelationshipProfile
              ? `I haven't seen enough of your mail with ${who.split(" ")[0]} to know how you write to them. Tell me roughly what you want to say and I'll put it in your words.`
              : "Tell me roughly what you want to say, or leave it blank and I'll answer from what they asked."}
          </p>
          <input
            type="text"
            aria-label={`Roughly what you want to say to ${who}`}
            placeholder="yes, happy to, same as last year"
            value={intent}
            onChange={(e) => setIntent(e.target.value)}
          />
          <div className="row gap-3">
            <Button variant="primary" onClick={writeDraft} disabled={generate.isPending}>
              {generate.isPending ? "Writing…" : "Write it"}
            </Button>
          </div>
        </div>
      )}
      {generate.isError ? <InlineError>{(generate.error as Error).message}</InlineError> : null}
    </article>
  );
}

/**
 * Use it, change it, or drop it. "Use this" copies the text and records that
 * you sent it as written; changing it records what you changed, which is the
 * only thing that teaches Mimic anything. Neither sends: Mimic has no send
 * path, deliberately, and adding one is a decision to make on purpose rather
 * than by accident.
 */
export function DraftReview({ draft, onDone }: { draft: Draft; onDone: () => void }) {
  const resolve = useResolveDraft();
  const [text, setText] = useState(draft.generatedText);
  const [editing, setEditing] = useState(false);
  const edited = text !== draft.generatedText;

  async function use() {
    await navigator.clipboard.writeText(text).catch(() => undefined);
    await resolve.mutateAsync({
      draftId: draft.id,
      outcome: edited ? "sent_edited" : "sent_unedited",
      finalText: text,
    });
    toast.success(
      edited ? "Copied. I noticed what you changed." : "Copied.",
      "Paste it into your email and send it — I don't send anything myself.",
    );
    onDone();
  }

  async function drop() {
    await resolve.mutateAsync({ draftId: draft.id, outcome: "discarded", finalText: null });
    onDone();
  }

  return (
    <div className="thread__part thread__part--ruled">
      <div className="letter-label letter-label--mine">I&rsquo;d say</div>
      {editing ? (
        <>
          <label htmlFor={`draft-${draft.id}`} className="muted small">
            Change whatever you like. I&rsquo;ll notice what you changed and write more like that
            next time.
          </label>
          <textarea
            id={`draft-${draft.id}`}
            className="letter letter--mine"
            rows={5}
            value={text}
            onChange={(e) => setText(e.target.value)}
            autoFocus
          />
        </>
      ) : (
        <p className="letter letter--mine">{text}</p>
      )}
      <div className="thread__actions">
        <Button variant="primary" onClick={use} disabled={resolve.isPending}>
          Use this
        </Button>
        <Button onClick={() => setEditing((v) => !v)}>
          {editing ? "Done changing" : "Change it"}
        </Button>
        <Button variant="ghost" onClick={drop} disabled={resolve.isPending}>
          Not this one
        </Button>
        <span className="spacer" />
        <button
          type="button"
          className="linkish"
          onClick={async () => {
            const note = window.prompt("What should I do differently next time?");
            if (!note?.trim()) return;
            await ipc.addDraftPreference(draft.id, note.trim());
            toast.success("Noted.", "What you tell me outweighs anything I guessed from an edit.");
          }}
        >
          Why I wrote it this way
        </button>
      </div>
      <p className="muted small">
        {draft.intent
          ? "Written from what you told me you wanted to say."
          : "You didn't tell me what you wanted to say, so this comes from their message and how you usually write — worth a read before you use it."}{" "}
        &ldquo;Use this&rdquo; copies it so you can paste it into your email. I never send anything.
      </p>
    </div>
  );
}
