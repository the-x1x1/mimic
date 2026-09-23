import { useState } from "react";
import { Button, Card, InlineError, ProgressBar } from "@mimic/ui";
import {
  JOB_KINDS,
  MEASURES,
  MEASURE_LABELS,
  SYSTEM_LABELS,
  describeEncoder,
  describeEvaluation,
  describeGone,
  describeSplitWarning,
  formatShare,
  type EvaluationView,
} from "@mimic/contracts";
import { useEvaluation, useStartEvaluation } from "@/hooks/useVoice";
import { useJobs } from "@/hooks/useJobs";
import { useProviderState } from "@/hooks/useCompose";
import { formatRelative } from "@/lib/format";

/** The most exchanges answered per run (mimic-core `evaluation::MAX_CASES`). */
export const CASES = 12;

/**
 * How close my drafts come to what you actually wrote, next to a generic
 * reply and the reply you send most often, on conversations I held back.
 * Every measure is shown on its own, with its 10th percentile, and nothing adds
 * them up: they are not the same kind of thing, so a single number would be
 * one nobody defined.
 */
export function HowClose() {
  const evaluation = useEvaluation();
  const start = useStartEvaluation();
  const jobs = useJobs();
  const providers = useProviderState();

  const view = evaluation.data ?? null;
  const runs = (jobs.data ?? []).filter((j) => j.type === JOB_KINDS.evaluateDrafts);
  const running = runs.find((j) => j.status === "queued" || j.status === "running");
  // The newest run failed since the measurement on screen: say why. An older
  // failure is history.
  const last = runs[0];
  const newer = last !== undefined && (!view || last.createdAt > view.createdAt);
  const failure =
    newer && last?.status === "failed"
      ? (last.error?.message ?? "It stopped without saying why.")
      : null;
  // Stopped by hand, or because something was deleted while it ran.
  const stopped = newer && last?.status === "canceled";

  const writer = providers.data?.providers.find((p) => p.id === providers.data?.active) ?? null;
  const asks = `It asks ${writer ? writer.displayName : "the writing model"} for two replies to each of up to ${CASES} messages people sent you.`;
  // What goes where, before anything is sent: I pick these messages, not you.
  const sends = !writer
    ? null
    : writer.local
      ? "Nothing leaves this computer."
      : `Those messages, the messages before each in its conversation, and some of your past replies with the messages they answered are sent to ${writer.displayName}, and billed like any other draft.`;
  const cost = [asks, sends, "Mail isn't checked until it's done."].filter(Boolean).join(" ");

  return (
    <Card
      title="How close my drafts come"
      actions={
        <Button
          size="sm"
          variant={view ? "ghost" : "primary"}
          disabled={running !== undefined || start.isPending}
          onClick={() => start.mutate()}
        >
          {view ? "Measure again" : "Measure it"}
        </Button>
      }
    >
      {view ? null : (
        <p className="neutral">
          I can check my drafts against what you actually wrote. I hold back some of your
          conversations, answer messages in them without looking at what you wrote back, and compare
          mine with what you sent &mdash; next to a generic reply from the same model, and the reply
          you send most often.
        </p>
      )}
      <p className="muted small">{cost}</p>
      {running ? (
        <ProgressBar
          current={running.progressCurrent}
          total={running.progressTotal}
          label={phaseLabel(running.phase)}
        />
      ) : null}
      {start.error ? <InlineError>{start.error.message}</InlineError> : null}
      {failure && !running ? <InlineError>{failure}</InlineError> : null}
      {stopped && !running ? (
        <p className="muted small">
          The last measurement was stopped before it finished &mdash; by you, or because mail was
          deleted while it ran &mdash; so nothing from it was kept.
        </p>
      ) : null}
      {view ? <Results view={view} /> : null}
    </Card>
  );
}

function phaseLabel(phase: string | null): string {
  if (!phase) return "Waiting to start";
  return phase.charAt(0).toUpperCase() + phase.slice(1);
}

function Results({ view }: { view: EvaluationView }) {
  const [showCases, setShowCases] = useState(false);
  const gone = describeGone(view);
  const encoder = describeEncoder(view.embeddingProvider);
  const measures = MEASURES.filter((m) => view.systems.some((s) => s[m] !== null));
  const theirs = (from: string | null) => (from ? `${from.split(" ")[0]} wrote` : "They wrote");

  return (
    <div className="stack gap-2">
      <p>
        {describeEvaluation(view)} <span className="muted">{formatRelative(view.createdAt)}</span>
      </p>
      {gone ? <p className="warn small">{gone}</p> : null}
      {view.remaining > 0 ? (
        <table className="table how-close">
          <thead>
            <tr>
              <th scope="col">Measure</th>
              {view.systems.map((s) => (
                <th key={s.system} scope="col" className="num">
                  {SYSTEM_LABELS[s.system]}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {measures.map((m) => (
              <tr key={m}>
                <th scope="row">
                  {MEASURE_LABELS[m].label}
                  <span className="muted small how-close__explains">
                    {MEASURE_LABELS[m].explains}
                  </span>
                </th>
                {view.systems.map((s) => {
                  const v = s[m];
                  return (
                    <td key={s.system} className="num">
                      {v ? (
                        <>
                          {formatShare(v.mean)}
                          <span className="muted small how-close__tail">
                            10th percentile {formatShare(v.p10)}
                          </span>
                        </>
                      ) : (
                        "not measured"
                      )}
                    </td>
                  );
                })}
              </tr>
            ))}
          </tbody>
        </table>
      ) : null}
      <ul className="plain-list muted small">
        <li>
          100% means the same as what you wrote in that respect. None of these says whether a reply
          was a good one.
        </li>
        <li>I wrote mine with no note from you, the way I write replies in advance.</li>
        <li>
          For this I worked out how you write again without the conversations I held back, and took
          no examples from them. I also left out what I&rsquo;ve learned from your edits to my
          drafts, since some of those edits could be to these very replies &mdash; that leans
          against me.
        </li>
        {encoder ? <li>{encoder}</li> : null}
        {view.commonReply ? (
          <li>
            Your most common reply is &ldquo;{view.commonReply.text}&rdquo;
            {view.commonReply.times && view.commonReply.times > 1
              ? `, sent ${view.commonReply.times} times.`
              : ". None of your replies repeats, so it's simply your most recent one."}
          </li>
        ) : null}
        {view.warnings.map((w) => (
          <li key={w}>{describeSplitWarning(w)}</li>
        ))}
      </ul>
      {view.cases.length > 0 ? (
        <div>
          <button type="button" className="linkish" onClick={() => setShowCases(!showCases)}>
            {showCases ? "Hide the replies" : "Show the replies"}
          </button>
        </div>
      ) : null}
      {showCases ? (
        <ol className="how-close__cases">
          {view.cases.map((c, i) => (
            <li key={i} className="thread__history-item">
              <div className="letter-label">{theirs(c.from)}</div>
              <p className="letter letter--theirs">{c.incoming}</p>
              <div className="letter-label letter-label--mine">You wrote</div>
              <p className="letter letter--mine">{c.reply}</p>
              {c.answers
                .filter((a) => a.system !== "common_reply")
                .map((a) => (
                  <div key={a.system} className="how-close__answer">
                    <div className="letter-label">
                      {a.system === "mimic" ? "I wrote" : SYSTEM_LABELS[a.system]}
                    </div>
                    <p className="letter">{a.text}</p>
                  </div>
                ))}
            </li>
          ))}
        </ol>
      ) : null}
    </div>
  );
}
