import { Button, Card, InlineError } from "@mimic/ui";
import { describeOutboxEntry } from "@mimic/contracts";
import { useReplyDidNotGo, useSent } from "@/hooks/useSending";
import { formatRelative } from "@/lib/format";

/**
 * Every reply Mimic sent, or tried to: newest first, with where it went and
 * whether it got there. A reply that didn't go says why, in the words the
 * mail server's answer gave. One that may have gone — the connection lost as
 * it was handed over — asks the user to look in their Sent folder, and only
 * once they say it isn't there can its draft be sent again. Nothing here
 * sends anything.
 */
export function SentList() {
  const sent = useSent();
  const didNotGo = useReplyDidNotGo();
  const entries = sent.data ?? [];
  if (entries.length === 0) return null;
  return (
    <Card title="What I've sent">
      <ul className="stack gap-2" aria-label="What I've sent">
        {entries.map((e) => (
          <li key={e.id} className="stack gap-1">
            <div className="row gap-2">
              <span>{e.subject}</span>
              <span className="muted small">{formatRelative(e.sentAt ?? e.createdAt)}</span>
            </div>
            <div className={e.status === "failed" ? "danger small" : "muted small"}>
              {describeOutboxEntry(e)}
            </div>
            {e.status === "unsure" ? (
              <div className="row gap-2">
                <Button
                  size="sm"
                  variant="ghost"
                  disabled={didNotGo.isPending}
                  onClick={() => didNotGo.mutate(e.id)}
                >
                  It isn&rsquo;t in my Sent folder
                </Button>
                <span className="muted small">Then you can send it again from its message.</span>
              </div>
            ) : null}
          </li>
        ))}
      </ul>
      {didNotGo.error ? <InlineError>{didNotGo.error.message}</InlineError> : null}
    </Card>
  );
}
