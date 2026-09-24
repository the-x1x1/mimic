import { Link } from "react-router-dom";
import { useState } from "react";
import { Badge, Button, Card, EmptyState, Metric } from "@mimic/ui";
import { MIN_SAMPLE, describeMetric, formatDuration, formatRate } from "@mimic/contracts";
import { PageHeader } from "@/components/PageHeader";
import {
  useAddVoiceNote,
  useForgetVoiceNote,
  useLearning,
  useStartAnalysis,
  useVoiceExamples,
  useVoiceOverview,
} from "@/hooks/useVoice";
import { useDraftOutcomes } from "@/hooks/useCompose";
import { countOf, formatRelative } from "@/lib/format";
import { HowClose } from "./HowClose";

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
        title="I haven't read anything you wrote yet"
        body="It learns only from messages you sent, not ones you received. Import a source, and make sure every address you write from is listed under Settings — a missing one makes your own messages look like someone else's."
        primary={
          <Link to="/sources" className="ui-btn ui-btn--primary ui-btn--md">
            Add a source
          </Link>
        }
      />
    );
  }

  return (
    <div className="stack gap-3">
      <PageHeader
        title="How you write"
        subtitle="Everything here I counted in mail you sent. Nothing here is a guess."
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

      <Learned />

      <HowClose />

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
    return <p className="neutral">I didn’t pick out any examples for this one.</p>;
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

/**
 * What the drafts you sent have taught Mimic, and what you told it outright.
 *
 * A pattern changes the next draft only once three drafts agree and outweigh
 * the drafts that went the other way; until then it is listed as forming,
 * with how far it has to go. Notes are listed with a way to take each back,
 * because something you said once should not be permanent by accident.
 */
function Learned() {
  const learning = useLearning();
  const add = useAddVoiceNote();
  const forget = useForgetVoiceNote();
  const [note, setNote] = useState("");
  const l = learning.data;
  if (!l) return null;
  const holding = l.patterns.filter((p) => p.holds);
  const forming = l.patterns.filter((p) => !p.holds);

  return (
    <Card title="What I've learned from you">
      {l.draftsConsidered === 0 ? (
        <p className="neutral">
          When you change one of my drafts before using it, I notice. Once you&rsquo;ve made the
          same change {l.minAgreeing} times &mdash; and not undone it more often than that &mdash; I
          start making it myself.
        </p>
      ) : (
        <>
          <p className="muted small">
            From {countOf(l.draftsConsidered, "draft")} you sent, edited or not.
          </p>
          {holding.length > 0 ? (
            <ul className="plain-list">
              {holding.map((p) => (
                <li key={`${p.habit}|${p.direction}|${p.participantId ?? ""}`}>{p.summary}</li>
              ))}
            </ul>
          ) : (
            <p className="neutral">Nothing you&rsquo;ve changed has added up to a habit yet.</p>
          )}
          {forming.length > 0 ? (
            <ul className="plain-list muted small">
              {forming.map((p) => (
                <li key={`${p.habit}|${p.direction}|${p.participantId ?? ""}`}>{p.summary}</li>
              ))}
            </ul>
          ) : null}
        </>
      )}

      <h3 className="card-subhead">Things you&rsquo;ve told me</h3>
      {l.notes.length === 0 ? (
        <p className="muted small">
          Nothing yet. What you tell me outranks anything I work out for myself.
        </p>
      ) : (
        <ul className="notes">
          {l.notes.map((n) => (
            <li key={n.id} className="notes__item">
              <span className="letter">{n.text}</span>
              <span className="muted small">
                {n.participantName ? `about ${n.participantName}` : "about everyone"}
              </span>
              <Button
                size="sm"
                variant="ghost"
                disabled={forget.isPending}
                onClick={() => forget.mutate(n.id)}
              >
                Forget this
              </Button>
            </li>
          ))}
        </ul>
      )}
      <form
        className="row gap-2 wrap"
        onSubmit={(e) => {
          e.preventDefault();
          if (!note.trim()) return;
          add.mutate({ participantId: null, note: note.trim() }, { onSuccess: () => setNote("") });
        }}
      >
        <input
          type="text"
          aria-label="Something I should always do, or never do"
          placeholder="I never use exclamation marks"
          maxLength={500}
          value={note}
          onChange={(e) => setNote(e.target.value)}
        />
        <Button type="submit" size="sm" disabled={add.isPending || !note.trim()}>
          Remember this
        </Button>
      </form>
    </Card>
  );
}
