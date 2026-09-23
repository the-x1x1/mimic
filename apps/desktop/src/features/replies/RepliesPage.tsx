import { useState } from "react";
import { useInfiniteQuery } from "@tanstack/react-query";
import { Link } from "react-router-dom";
import { Button, InlineError } from "@mimic/ui";
import {
  type Dashboard,
  type DashboardThread,
  type Draft,
  type ThreadMark,
  type Toward,
  SituationChoice,
  describeAutomated,
  describeLeftOut,
  describeQuiet,
  leftOutTotal,
  olderThan,
  describeMailChecking,
  describeSituation,
  describeWaiting,
  writerOf,
} from "@mimic/contracts";
import { useDashboard, useMarkThread, useStartAssistDrafts } from "@/hooks/useDashboard";
import { useGenerateDraft, useResolveDraft } from "@/hooks/useCompose";
import { useSituations } from "@/hooks/useVoice";
import { formatRelative } from "@/lib/format";
import { toast } from "@/state/toast";
import { ipc } from "@/lib/ipc";
import { qk } from "@/app/queryClient";
import { SentFolderNotice } from "@/features/identity/AddAddressForm";

/**
 * The one screen: who is waiting, what they said, and what Mimic would say
 * back.
 *
 * Two claims it must never make. It is not an inbox — every message on it
 * arrived through an import, and the first sentence says so before it says
 * anything else. And a draft is a draft: using one copies it and records what
 * you sent. Nothing here sends.
 *
 * What is waiting leaves out mail that looks automated, threads that have gone
 * quiet (older than the window in Settings) and threads the user said need no
 * reply. It always says how many it left out, and shows them when asked, so
 * leaving something out never becomes hiding it.
 */
export function RepliesPage() {
  const [showLeftOut, setShowLeftOut] = useState(false);
  const dashboard = useDashboard(25, showLeftOut);
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
            {data.awaiting.every((t) => t.draft !== null)
              ? "I've written a reply for each one, in your words."
              : "Tell me roughly what you want to say, and I'll put it in your words."}{" "}
            Read it, change anything you like, then send it yourself &mdash; I never send anything.
          </p>
        ) : null}
        <LeftOutLine data={data} showing={showLeftOut} onToggle={() => setShowLeftOut((v) => !v)} />
        {/* What the user sent from an address I wasn't given reads as someone
            waiting on them, so it is asked about here, where that shows. */}
        <SentFolderNotice />
      </div>

      {data.messages === 0 ? <NothingYet data={data} /> : null}
      {data.messages > 0 && data.awaiting.length === 0 ? <CaughtUp data={data} /> : null}

      <div className="replies__list">
        {data.awaiting.map((t) => (
          // Keyed by the message as well as the thread: when they write again,
          // everything this card remembered was about the old message.
          <Thread
            key={`${t.conversationId}:${t.lastMessageId}`}
            thread={t}
            withinDays={data.waitingWithinDays}
          />
        ))}
      </div>

      {showLeftOut && data.showingLeftOut && data.leftOutThreads.length > 0 ? (
        <LeftOutList data={data} />
      ) : null}

      {data.awaiting.length > 0 ? <Footer data={data} /> : null}
    </div>
  );
}

/** How many were left out and why, with the way to see them. Absent when none were. */
function LeftOutLine({
  data,
  showing,
  onToggle,
}: {
  data: Dashboard;
  showing: boolean;
  onToggle: () => void;
}) {
  const line = describeLeftOut(data);
  if (!line) return null;
  return (
    <p className="muted small">
      {line}{" "}
      <button
        type="button"
        className="linkish"
        onClick={onToggle}
        aria-expanded={showing}
        aria-controls={showing ? "left-out" : undefined}
      >
        {showing ? "Hide them" : "Show them"}
      </button>
    </p>
  );
}

/**
 * What was left out, grouped by why in the order the line above gives the
 * reasons, each group its own newest few — the core lists them per reason so
 * a month of newsletters cannot crowd out every thread that has gone quiet.
 */
export function LeftOutList({ data }: { data: Dashboard }) {
  const threads = data.leftOutThreads;
  const withinDays = data.waitingWithinDays;
  const groups = [
    {
      key: "automated",
      title:
        data.leftOut.automated === 1
          ? "This one looks automated to me"
          : "These look automated to me",
      items: threads.filter((t) => t.mark !== "no_reply_needed" && t.automated !== null),
      total: data.leftOut.automated,
    },
    {
      key: "quiet",
      title: `${data.leftOut.quiet === 1 ? "Its" : "Their"} last message is ${olderThan(withinDays)}`,
      items: threads.filter((t) => t.mark !== "no_reply_needed" && t.automated === null),
      total: data.leftOut.quiet,
    },
    {
      key: "not-needed",
      title:
        data.leftOut.notNeeded === 1
          ? "You said this one doesn't need a reply"
          : "You said these don't need a reply",
      items: threads.filter((t) => t.mark === "no_reply_needed"),
      total: data.leftOut.notNeeded,
    },
  ].filter((g) => g.items.length > 0);
  return (
    <section id="left-out" className="replies__leftout-list" aria-label="What I left out">
      <h2 className="card-subhead">What I left out</h2>
      <p className="muted small">
        As far as I can tell, none of these is waiting on you. If one is, say so and it goes back on
        the list.
      </p>
      {groups.map((g) => (
        <section key={g.key} aria-label={g.title}>
          <h3 className="letter-label">{g.title}</h3>
          {g.items.length < g.total ? (
            <p className="muted small">
              {g.items.length === 1
                ? `Here's the most recent of ${g.total.toLocaleString()}.`
                : `Here are the ${g.items.length} most recent of ${g.total.toLocaleString()}.`}
            </p>
          ) : null}
          {g.items.map((t) => (
            <LeftOutThread
              key={`${t.conversationId}:${t.lastMessageId}`}
              thread={t}
              withinDays={withinDays}
            />
          ))}
        </section>
      ))}
    </section>
  );
}

export function LeftOutThread({
  thread,
  withinDays,
}: {
  thread: DashboardThread;
  withinDays: number | null;
}) {
  const mark = useMarkThread();
  const who = thread.participant?.displayName ?? "Someone I couldn't put a name to";
  const address = thread.participant?.identifiers?.[0]?.value ?? null;
  const why =
    thread.mark === "no_reply_needed"
      ? "You said this one doesn't need a reply."
      : thread.automated !== null
        ? `It looks automated to me: ${describeAutomated(thread.automated) ?? ""}`
        : describeQuiet(withinDays);
  // Putting back a thread that also looks automated, or has gone quiet, has
  // to say it needs a reply: taking the mark away alone would leave it out
  // for the other reason.
  const back: ThreadMark | null = thread.automated !== null || thread.quiet ? "needs_reply" : null;
  return (
    <article className="thread thread--left-out">
      <header className="thread__head">
        <div className="thread__who">
          <span className="thread__name">{who}</span>
          {address ? <span className="thread__address">{address}</span> : null}
        </div>
        <span className="thread__when">{formatRelative(thread.lastMessageAt)}</span>
      </header>
      {thread.subject ? <p className="thread__subject">{thread.subject}</p> : null}
      <p className="letter letter--theirs thread__excerpt">{thread.lastMessage}</p>
      <p className="muted small">{why}</p>
      <div className="row gap-3">
        <Button
          size="sm"
          disabled={mark.isPending}
          aria-label={`${thread.mark === "no_reply_needed" ? "Put it back" : "It needs a reply"}: ${who}`}
          onClick={() =>
            mark.mutate({
              conversationId: thread.conversationId,
              messageId: thread.lastMessageId,
              mark: back,
            })
          }
        >
          {thread.mark === "no_reply_needed" ? "Put it back" : "It needs a reply"}
        </Button>
      </div>
    </article>
  );
}

/**
 * The quiet way to say a thread needs no answer. It is tied to the message on
 * screen, so the thread comes back by itself if they write again.
 */
function NoReplyNeeded({ thread, who }: { thread: DashboardThread; who: string }) {
  const mark = useMarkThread();
  return (
    <button
      type="button"
      className="linkish"
      disabled={mark.isPending}
      aria-label={`Doesn't need a reply: ${who}`}
      onClick={() =>
        mark.mutate(
          {
            conversationId: thread.conversationId,
            messageId: thread.lastMessageId,
            mark: "no_reply_needed",
          },
          {
            onSuccess: (applied) => {
              if (applied) {
                toast.info(
                  "Taken off the list.",
                  "It comes back if they write again. Until then it's with what I left out.",
                );
              }
            },
          },
        )
      }
    >
      Doesn&rsquo;t need a reply
    </button>
  );
}

function NothingYet({ data }: { data: Dashboard }) {
  if (data.mailChecking?.failing) {
    return (
      <section className="note">
        <h2 className="note__title">I couldn&rsquo;t read your mail.</h2>
        <p className="note__body">{describeMailChecking(data)}</p>
        <div className="note__actions">
          <Link to="/sources">
            <Button variant="primary">See your mail</Button>
          </Link>
        </div>
      </section>
    );
  }
  if (data.mailChecking) {
    return (
      <section className="note">
        <h2 className="note__title">I&rsquo;m reading your mail.</h2>
        <p className="note__body">
          This fills up as I go. Nothing in your mailbox changes, and nothing leaves this computer
          except what goes to the writing model you chose.
        </p>
      </section>
    );
  }
  return (
    <section className="note">
      <h2 className="note__title">There&rsquo;s nothing here yet.</h2>
      <p className="note__body">
        I haven&rsquo;t read any of your mail, so I don&rsquo;t know how you write or who you write
        to. Connect your mailbox, or point me at a copy of your old mail, and this fills up. Nothing
        leaves this computer, and nothing arrives on its own &mdash; I only read what you hand me.
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

function CaughtUp({ data }: { data: Dashboard }) {
  const checking = describeMailChecking(data);
  return (
    <section className="note">
      <h2 className="note__title">You&rsquo;re all caught up.</h2>
      <p className="note__body">
        {leftOutTotal(data) > 0
          ? "Everything I've read either ends with you, or is one of the threads I left out."
          : "Nothing in the mail I've read ends with someone else waiting on you."}{" "}
        {checking && data.mailChecking?.failing
          ? checking
          : checking && data.mailChecking?.everyMinutes
            ? `${checking} Anything new that needs an answer will show up here.`
            : "Bring in newer mail and I'll have replies ready for anything new."}
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
        {describeMailChecking(data) ? `${describeMailChecking(data)} ` : null}
        {data.autoDraft
          ? `I write these by myself after each ${data.mailChecking ? "check" : "import"}. If you would rather I asked first, turn that off in Settings.`
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
export function Thread({
  thread,
  withinDays = null,
}: {
  thread: DashboardThread;
  /** The waiting window, to name when the user kept an old thread on the list. */
  withinDays?: number | null;
}) {
  const generate = useGenerateDraft();
  // A draft written here shows at once; one prepared in the background shows
  // when the screen next loads it; one the user has used or dropped stays
  // gone until the screen catches up.
  const [written, setWritten] = useState<Draft | null>(null);
  const [doneWith, setDoneWith] = useState<string | null>(null);
  const candidate = written ?? thread.draft;
  const draft = candidate && candidate.id !== doneWith ? candidate : null;
  const [intent, setIntent] = useState("");
  // Empty means "work it out from my note", which is the default: most people
  // will never touch this, and the note is read the same way either way.
  const [situationId, setSituationId] = useState("");
  const situations = useSituations();
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
      incomingMessageId: thread.lastMessageId,
      intent: intent.trim() || null,
      situationId: situationId || null,
    });
    setWritten(result);
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

      {thread.mark === "needs_reply" && (thread.automated !== null || thread.quiet) ? (
        <KeptOnTheList thread={thread} withinDays={withinDays} />
      ) : null}

      {thread.earlier > 0 ? (
        <RestOfThread
          // Another message on screen is another place to read from.
          key={`earlier:${thread.lastMessageId}`}
          conversationId={thread.conversationId}
          messageId={thread.lastMessageId}
          toward="earlier"
          count={thread.earlier}
        />
      ) : null}

      <div className="thread__part">
        <div className="letter-label">{saidLabel}</div>
        <p className="letter letter--theirs">{thread.lastMessage}</p>
        <div className="thread__aside">
          <NoReplyNeeded thread={thread} who={who} />
        </div>
      </div>

      {thread.later > 0 ? (
        <RestOfThread
          key={`later:${thread.lastMessageId}`}
          conversationId={thread.conversationId}
          messageId={thread.lastMessageId}
          toward="later"
          count={thread.later}
        />
      ) : null}

      {draft ? (
        <DraftReview
          // Its text and buttons must be about the same draft, so a different
          // draft is a different panel.
          key={draft.id}
          draft={draft}
          onDone={() => {
            setDoneWith(draft.id);
            setWritten(null);
          }}
        />
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
          {situations.data ? (
            <label className="thread__kind">
              <span className="muted small">What kind of reply</span>
              <select value={situationId} onChange={(e) => setSituationId(e.target.value)}>
                <option value="">Work it out from what I wrote</option>
                {situations.data.map((s) => (
                  <option key={s.id} value={s.id}>
                    {s.label}
                    {s.measurable ? "" : " (I haven't seen enough of these yet)"}
                  </option>
                ))}
              </select>
            </label>
          ) : null}
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

/** Messages of the conversation read in at a time. */
const PAGE = 20;

/**
 * Said when a message a page is read from is no longer there — its person or
 * its mail was deleted while the conversation was open. Which one isn't
 * known here (the card's message, or the far end of a page already shown),
 * so it names neither, and what was shown is put away rather than left on
 * screen under a line saying part of it is gone.
 */
export const GONE = "Part of this conversation isn't here any more.";

function isGone(e: unknown): boolean {
  return (e as { code?: unknown } | null)?.code === "not_found";
}

/**
 * The rest of the conversation the message on screen is part of, on one side
 * of it, when the user asks for it: oldest first, a page at a time. Closed by
 * default, because the message itself is usually enough, and read only when
 * opened. Before it is the history. After it is whatever came in that did not
 * decide whether the thread is waiting — something that looks automated, or
 * whose writer could not be told — shown so that the card hides nothing.
 */
export function RestOfThread({
  conversationId,
  messageId,
  toward,
  count,
}: {
  conversationId: string;
  messageId: string;
  toward: Toward;
  /** How many messages are on that side, as the home screen counted them. */
  count: number;
}) {
  const [open, setOpen] = useState(false);
  const earlier = toward === "earlier";
  const pages = useInfiniteQuery({
    queryKey: qk.conversation(conversationId, messageId, toward),
    queryFn: ({ pageParam }) => ipc.conversationPage(conversationId, pageParam, toward, PAGE),
    initialPageParam: messageId,
    // Read on from the far end of the last page read.
    getNextPageParam: (last) => {
      const far = earlier ? last.messages[0] : last.messages.at(-1);
      return last.more > 0 && far ? far.id : undefined;
    },
    enabled: open,
  });
  const side = earlier ? "before" : "after";

  if (!open) {
    return (
      <div className="thread__rest">
        <button type="button" className="linkish" onClick={() => setOpen(true)}>
          {count === 1
            ? `Show the message ${side} this one`
            : `Show the ${count.toLocaleString()} messages ${side} this one`}
        </button>
      </div>
    );
  }

  const read = pages.data?.pages ?? [];
  // Pages before it come nearest first, so the furthest back goes on top.
  const messages = (earlier ? [...read].reverse() : read).flatMap((p) => p.messages);
  const more = read.at(-1)?.more ?? 0;
  const further =
    more > 0 ? (
      <button
        type="button"
        className="linkish"
        disabled={pages.isFetchingNextPage}
        onClick={() => void pages.fetchNextPage()}
      >
        {pages.isFetchingNextPage
          ? earlier
            ? "Reading further back…"
            : "Reading further on…"
          : `Show ${more.toLocaleString()} ${earlier ? "earlier" : "later"} ${more === 1 ? "message" : "messages"}`}
      </button>
    ) : null;

  const gone = pages.isError && isGone(pages.error);
  if (gone) {
    return (
      <div className="thread__rest">
        <button type="button" className="linkish" onClick={() => setOpen(false)}>
          Hide what came {side}
        </button>
        <p className="muted small row gap-2">
          <span>{GONE}</span>
          <button
            type="button"
            className="linkish"
            disabled={pages.isFetching}
            onClick={() => void pages.refetch()}
          >
            Read it again
          </button>
        </p>
      </div>
    );
  }

  return (
    <div className="thread__rest">
      <button type="button" className="linkish" onClick={() => setOpen(false)}>
        Hide what came {side}
      </button>
      {pages.isError ? <InlineError>{pages.error.message}</InlineError> : null}
      {pages.isPending ? <p className="muted small">Reading it back…</p> : null}
      {earlier ? further : null}
      {messages.length > 0 ? (
        <ol className={earlier ? "thread__history" : "thread__history thread__history--later"}>
          {messages.map((m) => (
            <li key={m.id} className="thread__history-item">
              <div
                className={
                  m.direction === "self" ? "letter-label letter-label--mine" : "letter-label"
                }
              >
                {writerOf(m)}
                {m.sentAt ? <span className="muted"> · {formatRelative(m.sentAt)}</span> : null}
              </div>
              <p
                className={m.direction === "self" ? "letter letter--mine" : "letter letter--theirs"}
              >
                {m.body}
              </p>
              {m.automated !== null ? (
                <p className="muted small">
                  It looks automated to me: {describeAutomated(m.automated)}
                </p>
              ) : null}
            </li>
          ))}
        </ol>
      ) : null}
      {earlier ? null : further}
    </div>
  );
}

/**
 * A thread the user kept on the list although it looks automated or has gone
 * quiet, and the way to undo that.
 */
function KeptOnTheList({
  thread,
  withinDays,
}: {
  thread: DashboardThread;
  withinDays: number | null;
}) {
  const mark = useMarkThread();
  const who = thread.participant?.displayName ?? "Someone I couldn't put a name to";
  return (
    <p className="muted small">
      {thread.automated !== null
        ? `You said this one needs a reply, though it looks automated to me: ${describeAutomated(thread.automated) ?? ""}`
        : `You said this one needs a reply, though its last message is ${olderThan(withinDays)}.`}{" "}
      <button
        type="button"
        className="linkish"
        aria-label={`Take that back: ${who}`}
        disabled={mark.isPending}
        onClick={() =>
          mark.mutate({
            conversationId: thread.conversationId,
            messageId: thread.lastMessageId,
            mark: null,
          })
        }
      >
        Take that back
      </button>
    </p>
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
  const [telling, setTelling] = useState(false);
  const [note, setNote] = useState("");
  const [savingNote, setSavingNote] = useState(false);
  const edited = text !== draft.generatedText;
  const situation = SituationChoice.safeParse(draft.context["situation"]);
  const situationLine = describeSituation(situation.success ? situation.data : null);

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
        <button type="button" className="linkish" onClick={() => setTelling((v) => !v)}>
          {telling ? "Never mind" : "Tell me what to do differently"}
        </button>
      </div>
      {telling ? (
        <form
          className="thread__tell"
          onSubmit={async (e) => {
            e.preventDefault();
            if (!note.trim()) return;
            setSavingNote(true);
            try {
              await ipc.addDraftPreference(draft.id, note.trim());
              toast.success(
                "I'll remember that.",
                "What you tell me outranks anything I worked out for myself. You can take it back under How you write.",
              );
              setNote("");
              setTelling(false);
            } catch (err) {
              toast.danger("I couldn't keep that", (err as Error).message);
            } finally {
              setSavingNote(false);
            }
          }}
        >
          <label htmlFor={`tell-${draft.id}`} className="muted small">
            {draft.participantId
              ? "I'll remember this for everything I write to them."
              : "I'll remember this for everything I write."}
          </label>
          <input
            id={`tell-${draft.id}`}
            type="text"
            maxLength={500}
            placeholder="never sign off with 'best'"
            value={note}
            onChange={(e) => setNote(e.target.value)}
            autoFocus
          />
          <div className="row gap-3">
            <Button size="sm" type="submit" disabled={savingNote || !note.trim()}>
              {savingNote ? "Keeping it…" : "Remember this"}
            </Button>
          </div>
        </form>
      ) : null}
      <p className="muted small">
        {situationLine ? `${situationLine} ` : null}
        {draft.intent
          ? "Written from what you told me you wanted to say."
          : "You didn't tell me what you wanted to say, so this comes from their message and how you usually write — worth a read before you use it."}{" "}
        &ldquo;Use this&rdquo; copies it so you can paste it into your email. I never send anything.
      </p>
    </div>
  );
}
