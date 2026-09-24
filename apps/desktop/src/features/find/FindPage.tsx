import { useEffect, useId, useRef, useState, type FormEvent } from "react";
import { useInfiniteQuery } from "@tanstack/react-query";
import { Button, InlineError } from "@mimic/ui";
import {
  describeFound,
  hasWordsToFind,
  writerOf,
  type FoundMessage,
  type SearchWho,
} from "@mimic/contracts";
import { PageHeader } from "@/components/PageHeader";
import { ipc } from "@/lib/ipc";
import { qk } from "@/app/queryClient";
import { formatRelative } from "@/lib/format";
import { RestOfThread } from "@/features/replies/RepliesPage";
import { ThreadMessages } from "@/features/replies/ThreadMessages";

const PAGE = 20;

const WHO: { value: SearchWho; label: string }[] = [
  { value: "anyone", label: "Anyone" },
  { value: "you", label: "You" },
  // Every message that is not the user's, those whose writer could not be told too.
  { value: "others", label: "Not you" },
];

/**
 * Finding what was said: every message Mimic has read, the user's and
 * everyone else's, searched by the words in it, newest first — each with the
 * words around the match, and its conversation either side of it on request.
 * The search happens here, in the database on this computer.
 */
export function FindPage() {
  const [typed, setTyped] = useState("");
  const [who, setWho] = useState<SearchWho>("anyone");
  const [asked, setAsked] = useState<{ query: string; who: SearchWho } | null>(null);
  const [nothingToFind, setNothingToFind] = useState(false);
  const inputId = useId();
  const whoId = useId();
  const list = useRef<HTMLOListElement>(null);
  // Where the next page starts, once asked for: the keyboard goes there when
  // it arrives, since the button it was on goes on the last page.
  const focusFrom = useRef<number | null>(null);

  const pages = useInfiniteQuery({
    queryKey: qk.search(asked?.query ?? "", asked?.who ?? "anyone"),
    queryFn: ({ pageParam }) =>
      ipc.searchMessages(asked?.query ?? "", asked?.who ?? "anyone", pageParam, PAGE),
    initialPageParam: null as { at: string; id: string } | null,
    // Read on from the last message shown.
    getNextPageParam: (last) => {
      const end = last.found.at(-1);
      return last.more && end ? { at: end.message.sentAt ?? "", id: end.message.id } : undefined;
    },
    enabled: asked !== null,
  });

  const submit = (e: FormEvent) => {
    e.preventDefault();
    const query = typed.trim();
    if (!hasWordsToFind(query)) {
      setAsked(null);
      setNothingToFind(true);
      return;
    }
    setNothingToFind(false);
    setAsked({ query, who });
  };
  const chooseWho = (next: SearchWho) => {
    setWho(next);
    // A search already made is made again for whoever is chosen.
    if (asked) setAsked({ ...asked, who: next });
  };

  const read = pages.data?.pages ?? [];
  const found = read.flatMap((p) => p.found);
  const first = read[0];

  useEffect(() => {
    const from = focusFrom.current;
    if (from === null || found.length <= from) return;
    focusFrom.current = null;
    (list.current?.children[from] as HTMLElement | undefined)?.focus();
  }, [found.length]);

  // One live region, always there, so each change to it is announced.
  const status = nothingToFind
    ? "Type a word or two to look for."
    : asked && pages.isPending
      ? "Looking…"
      : asked && first
        ? describeFound(first, asked.who)
        : "";

  return (
    <div className="stack gap-3">
      <PageHeader
        title="Find"
        subtitle="Anything said in the mail I've read — by you or to you. Looking happens on this computer."
      />
      <form className="stack gap-2" role="search" aria-label="Find what was said" onSubmit={submit}>
        <div className="row gap-2 wrap">
          <div className="stack gap-1 find__words">
            <label className="small" htmlFor={inputId}>
              Words to find
            </label>
            <input
              id={inputId}
              type="search"
              value={typed}
              onChange={(e) => setTyped(e.target.value)}
              onKeyDown={(e) => {
                // Escape clears the words, as in any search field, and is
                // handled: only an Escape with nothing to clear closes Find.
                if (e.key === "Escape" && typed !== "") {
                  e.preventDefault();
                  setTyped("");
                }
              }}
              placeholder="drinks friday"
              autoComplete="off"
              spellCheck={false}
            />
          </div>
          <div className="stack gap-1">
            <label className="small" htmlFor={whoId}>
              Written by
            </label>
            <select id={whoId} value={who} onChange={(e) => chooseWho(e.target.value as SearchWho)}>
              {WHO.map((w) => (
                <option key={w.value} value={w.value}>
                  {w.label}
                </option>
              ))}
            </select>
          </div>
          <div className="find__go">
            <Button
              type="submit"
              variant="primary"
              disabled={pages.isFetching && !pages.isFetchingNextPage}
            >
              Find
            </Button>
          </div>
        </div>
        <p className="muted small">
          Every word has to be there, in any order; &ldquo;words in quotes&rdquo; have to be
          together. Capitals and accents don&rsquo;t matter, and a last word of three letters or
          more can be the start of a longer one.
        </p>
      </form>

      <p role="status" className={nothingToFind || pages.isPending ? "muted" : undefined}>
        {status}
      </p>
      {pages.isError ? <InlineError>{pages.error.message}</InlineError> : null}
      {found.length > 0 ? (
        <ol ref={list} className="stack gap-3 found">
          {found.map((f) => (
            <FoundItem key={f.message.id} f={f} />
          ))}
        </ol>
      ) : null}
      {read.at(-1)?.more ? (
        <button
          type="button"
          className="linkish"
          disabled={pages.isFetchingNextPage}
          onClick={() => {
            focusFrom.current = found.length;
            void pages.fetchNextPage();
          }}
        >
          {pages.isFetchingNextPage ? "Looking further back…" : "Show older ones"}
        </button>
      ) : null}
    </div>
  );
}

/**
 * One message found: who wrote it, when, in which conversation, the words
 * around the match; and, when asked, the message whole with its conversation
 * either side of it.
 */
function FoundItem({ f }: { f: FoundMessage }) {
  const [open, setOpen] = useState(false);
  const subject = f.subject?.trim() || "No subject";
  const region = `found-${f.message.id}`;
  const about = `${region}-about`;
  const facts = [
    f.message.sentAt ? formatRelative(f.message.sentAt) : null,
    `in ${subject}`,
  ].filter((x): x is string => x !== null);
  return (
    <li className="stack gap-1" tabIndex={-1}>
      <div
        id={about}
        className={
          f.message.direction === "self" ? "letter-label letter-label--mine" : "letter-label"
        }
      >
        {writerOf(f.message)}
        <span className="muted"> · {facts.join(" · ")}</span>
      </div>
      <p className="found__snippet">
        {f.snippet.map((p, i) =>
          p.hit ? <mark key={i}>{p.text}</mark> : <span key={i}>{p.text}</span>,
        )}
      </p>
      <div>
        <button
          type="button"
          className="linkish"
          // Which message it is about, since every result has one.
          aria-describedby={about}
          aria-controls={open ? region : undefined}
          onClick={() => setOpen((o) => !o)}
        >
          {open ? "Hide the conversation" : "Show it in its conversation"}
        </button>
      </div>
      {open ? (
        <div id={region} className="thread">
          {f.earlier > 0 ? (
            <RestOfThread
              conversationId={f.conversationId}
              messageId={f.message.id}
              toward="earlier"
              count={f.earlier}
            />
          ) : null}
          <ThreadMessages messages={[f.message]} />
          {f.later > 0 ? (
            <RestOfThread
              conversationId={f.conversationId}
              messageId={f.message.id}
              toward="later"
              count={f.later}
            />
          ) : null}
        </div>
      ) : null}
    </li>
  );
}
