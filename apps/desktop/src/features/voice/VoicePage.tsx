import { useState } from "react";
import { Badge, Button, Card, EmptyState, Metric } from "@mimic/ui";
import { MIN_SAMPLE, describeMetric, formatDuration, formatRate } from "@mimic/contracts";
import { PageHeader } from "@/components/PageHeader";
import { useStartAnalysis, useVoiceExamples, useVoiceOverview } from "@/hooks/useVoice";
import { useDraftOutcomes } from "@/hooks/useCompose";
import { countOf, formatRelative } from "@/lib/format";

const HABITS = [
  "medianWordsPerMessage",
  "terminalPeriodRate",
  "lowercaseStartRate",
  "emojiRate",
  "greetingRate",
  "signOffRate",
] as const;

/**
 * What Mimic has worked out, and — just as prominently — what it has not.
 *
 * A layer with too few messages is shown with its sample size and no numbers.
 * That is the whole design: an empty chart is a lie, "14 of the 20 messages
 * needed" is not.
 */
export function VoicePage() {
  const overview = useVoiceOverview();
  const analyze = useStartAnalysis();
  const outcomes = useDraftOutcomes();
  const [open, setOpen] = useState<string | null>("global|");

  const o = overview.data;
  if (overview.isSuccess && o!.ownMessages === 0) {
    return (
      <EmptyState
        title="Mimic has not read anything you wrote"
        body="It learns only from messages you sent, not ones you received. Import a source and make sure the addresses you write from are listed under Settings."
      />
    );
  }

  return (
    <div className="stack gap-3">
      <PageHeader
        title="Voice"
        subtitle="Everything here was measured from your own messages. Nothing is estimated."
        actions={
          <Button variant="primary" onClick={() => analyze.mutate()} disabled={analyze.isPending}>
            {o?.stale ? "Re-analyze" : "Analyze again"}
          </Button>
        }
      />

      <div className="metric-grid">
        <Metric
          label="Your messages analyzed"
          value={(o?.ownMessages ?? 0).toLocaleString()}
          hint={
            o && o.messagesUntilMeasurable > 0
              ? `${countOf(o.messagesUntilMeasurable, "more")} before anything can be measured`
              : "enough to describe how you write"
          }
          tone={o && o.messagesUntilMeasurable > 0 ? "warn" : "good"}
        />
        <Metric
          label="People with their own profile"
          value={o?.peopleWithProfiles ?? 0}
          hint={`needs ${MIN_SAMPLE} messages you wrote to them`}
        />
        <Metric
          label="Last analyzed"
          value={formatRelative(o?.lastAnalyzedAt)}
          hint={o?.stale ? "out of date — messages have arrived since" : "up to date"}
          tone={o?.stale ? "warn" : "neutral"}
        />
        <Metric
          label="Drafts you sent unchanged"
          value={formatRate(outcomes.data?.uneditedRate ?? null)}
          hint="measured from drafts you told Mimic about"
          tone={outcomes.data?.uneditedRate === null ? "neutral" : "good"}
        />
      </div>

      {(o?.profiles ?? []).map((p) => {
        const key = `${p.layer}|${p.scopeKey}`;
        const habits = HABITS.map((h) => describeMetric(h, p.metrics)).filter(
          (s): s is string => s !== null,
        );
        return (
          <Card key={key} title={p.label || `${p.layer} ${p.scopeKey}`}>
            <div className="row gap-2 wrap">
              <Badge tone={p.measurable ? "success" : "neutral"}>
                {countOf(p.sampleSize, "message")}
              </Badge>
              {p.stale ? <Badge tone="warning">Out of date</Badge> : null}
              <Button
                size="sm"
                variant="ghost"
                onClick={() => setOpen(open === key ? null : key)}
                disabled={!p.measurable}
              >
                {open === key ? "Hide examples" : "Show examples"}
              </Button>
            </div>

            {p.measurable ? (
              <>
                <ul className="plain-list">
                  {habits.map((line) => (
                    <li key={line}>{line}</li>
                  ))}
                  {p.metrics.medianResponseSeconds !== null ? (
                    <li>
                      You usually reply within {formatDuration(p.metrics.medianResponseSeconds)}
                    </li>
                  ) : null}
                </ul>
                {p.metrics.topPhrases.length > 0 ? (
                  <p className="muted small">
                    Phrases you repeat:{" "}
                    {p.metrics.topPhrases
                      .slice(0, 6)
                      .map(([phrase, n]) => `“${phrase}” (${n}×)`)
                      .join(", ")}
                  </p>
                ) : null}
              </>
            ) : (
              <p className="neutral">
                {countOf(p.sampleSize, "message")} is not enough to describe a style. Mimic needs at
                least {MIN_SAMPLE} before it will say anything about this one.
              </p>
            )}

            {open === key && p.measurable ? (
              <Examples layer={p.layer} scopeKey={p.scopeKey} />
            ) : null}
          </Card>
        );
      })}
    </div>
  );
}

function Examples({ layer, scopeKey }: { layer: string; scopeKey: string }) {
  const examples = useVoiceExamples(layer, scopeKey);
  if (examples.isLoading) return <p className="neutral">Loading…</p>;
  if (!examples.data?.length)
    return <p className="neutral">No examples were selected for this one.</p>;
  return (
    <ul className="examples">
      {examples.data.map((e) => (
        <li key={e.id}>
          <p className="examples__reply">{e.body}</p>
          <p className="muted small">
            {e.reason} · {formatRelative(e.sentAt)}
          </p>
        </li>
      ))}
    </ul>
  );
}
