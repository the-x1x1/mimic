import { describeAutomated, writerOf, type ThreadMessage } from "@mimic/contracts";
import { formatRelative } from "@/lib/format";

/**
 * Messages of a conversation, oldest first, each labelled with who wrote it
 * — the user's own as theirs, and nobody guessed — and, where its headers say
 * so, that it looks automated.
 */
export function ThreadMessages({
  messages,
  later = false,
}: {
  messages: ThreadMessage[];
  /** What came after the message on a card, set apart from what came before. */
  later?: boolean;
}) {
  return (
    <ol className={later ? "thread__history thread__history--later" : "thread__history"}>
      {messages.map((m) => (
        <li key={m.id} className="thread__history-item">
          <div
            className={m.direction === "self" ? "letter-label letter-label--mine" : "letter-label"}
          >
            {writerOf(m)}
            {m.sentAt ? <span className="muted"> · {formatRelative(m.sentAt)}</span> : null}
          </div>
          <p className={m.direction === "self" ? "letter letter--mine" : "letter letter--theirs"}>
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
  );
}
