import { Card } from "@mimic/ui";
import type { GenerationContext } from "@mimic/contracts";
import { describeMetric, formatDuration } from "@mimic/contracts";

/**
 * What a draft is based on, in the user's terms. This panel exists because the
 * alternative — a box that produces text with no account of itself — is the
 * thing that makes people distrust a tool like this.
 *
 * Every line here comes from a measurement or a row. Nothing is inferred for
 * display purposes.
 */
export function EvidencePanel({
  context,
  loading,
}: {
  context: GenerationContext | undefined;
  loading: boolean;
}) {
  if (loading) return <Card title="What this is based on">Reading your messages…</Card>;
  if (!context) return null;

  const m = context.effective;
  const habits = (
    [
      "medianWordsPerMessage",
      "terminalPeriodRate",
      "lowercaseStartRate",
      "emojiRate",
      "greetingRate",
      "signOffRate",
    ] as const
  )
    .map((k) => describeMetric(k, m))
    .filter((s): s is string => s !== null);

  return (
    <>
      <Card title="What this is based on">
        {context.evidence.length === 0 ? (
          <p className="neutral">Nothing yet.</p>
        ) : (
          <ul className="plain-list">
            {context.evidence.map((line) => (
              <li key={line}>{line}</li>
            ))}
          </ul>
        )}
      </Card>

      {habits.length > 0 ? (
        <Card title="How you write, measured">
          <ul className="plain-list">
            {habits.map((line) => (
              <li key={line}>{line}</li>
            ))}
            {m.medianResponseSeconds !== null ? (
              <li>You usually reply within {formatDuration(m.medianResponseSeconds)}</li>
            ) : null}
          </ul>
          {m.topPhrases.length > 0 ? (
            <p className="muted small">
              Phrases you repeat:{" "}
              {m.topPhrases
                .slice(0, 5)
                .map(([p]) => `“${p}”`)
                .join(", ")}
            </p>
          ) : null}
        </Card>
      ) : null}

      {context.examples.length > 0 ? (
        <Card title="Your own messages being used">
          <ul className="examples">
            {context.examples.map((e) => (
              <li key={e.replyMessageId}>
                {e.incoming ? <p className="examples__incoming">{e.incoming}</p> : null}
                <p className="examples__reply">{e.reply}</p>
                <p className="muted small">{e.reason}</p>
              </li>
            ))}
          </ul>
        </Card>
      ) : null}
    </>
  );
}
