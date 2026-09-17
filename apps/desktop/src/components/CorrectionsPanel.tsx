import { Badge, Card, EmptyState, Metric } from "@mimic/ui";
import {
  controlLabel,
  formatNoTouch,
  type CorrectionRow,
  type StyleHealth,
} from "@mimic/contracts";
import { formatDate } from "@/lib/format";

function fmtRaw(v: unknown): string {
  if (typeof v === "number") return Number.isInteger(v) ? String(v) : v.toFixed(2);
  if (v === null || v === undefined) return "—";
  return String(v);
}

/**
 * Corrections tab: measured No-Touch Rate per version, what gets corrected
 * most, and the individual corrections. Every number comes from
 * `corrections` / `correction_syncs` rows; nothing is estimated.
 */
export function CorrectionsPanel({
  health,
  corrections,
}: {
  health: StyleHealth;
  corrections: CorrectionRow[] | undefined;
}) {
  const measured = health.noTouch.filter((n) => n.rate !== null);
  if (health.correctionsTotal === 0 && measured.length === 0) {
    return (
      <EmptyState
        title="No corrections synced yet"
        body="After you apply a session and finish your own pass in Lightroom, open the session and choose “Sync corrections”. Mimic reads each applied photo back, keeps what you changed as a correction, and counts what you left alone as the No-Touch Rate."
      />
    );
  }
  return (
    <div className="stack gap-3">
      <div className="metric-grid">
        <Metric
          label="No-Touch Rate (active version)"
          value={formatNoTouch(health.activeNoTouchRate)}
          hint="applied photos you left untouched after sync"
          tone={
            health.activeNoTouchRate === null
              ? "neutral"
              : health.activeNoTouchRate >= 0.7
                ? "good"
                : "warn"
          }
        />
        <Metric
          label="Corrections"
          value={health.correctionsTotal}
          hint="photos you changed after an apply"
        />
        <Metric
          label="Waiting for training"
          value={health.correctionsPendingTraining}
          hint="included by the next Train New Version"
          tone={health.correctionsPendingTraining > 0 ? "warn" : "neutral"}
        />
      </div>
      {health.insights.length > 0 ? (
        <Card title="What the corrections say">
          <ul className="plain-list">
            {health.insights.map((i) => (
              <li key={i}>{i}</li>
            ))}
          </ul>
        </Card>
      ) : null}
      {measured.length > 0 ? (
        <Card title="No-Touch Rate by version">
          <table className="table">
            <thead>
              <tr>
                <th>Version</th>
                <th className="num">Checked</th>
                <th className="num">Untouched</th>
                <th className="num">Corrected</th>
                <th className="num">No-Touch</th>
              </tr>
            </thead>
            <tbody>
              {health.noTouch.map((n) => (
                <tr key={n.modelVersionId}>
                  <td>v{n.semanticVersion}</td>
                  <td className="num">{n.appliedChecked}</td>
                  <td className="num">{n.untouched}</td>
                  <td className="num">{n.corrected}</td>
                  <td className="num">{formatNoTouch(n.rate)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </Card>
      ) : null}
      {health.mostCorrected.length > 0 ? (
        <Card title="Most corrected controls">
          <table className="table">
            <thead>
              <tr>
                <th>Control</th>
                <th className="num">Photos</th>
                <th className="num">Mean |Δ| (of range)</th>
                <th className="num">Bias</th>
              </tr>
            </thead>
            <tbody>
              {health.mostCorrected.map((c) => (
                <tr key={c.canonical}>
                  <td>{controlLabel(c.canonical)}</td>
                  <td className="num">{c.corrections}</td>
                  <td className="num">{(c.meanAbsDelta * 100).toFixed(1)}%</td>
                  <td className="num">
                    {c.meanDelta > 0 ? "+" : ""}
                    {(c.meanDelta * 100).toFixed(1)}%
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </Card>
      ) : null}
      {corrections && corrections.length > 0 ? (
        <Card title={`Corrections (${corrections.length})`}>
          <table className="table">
            <thead>
              <tr>
                <th>Photo</th>
                <th>Changed</th>
                <th className="num">Magnitude</th>
                <th>Model</th>
                <th>When</th>
              </tr>
            </thead>
            <tbody>
              {corrections.map((c) => (
                <tr key={c.id}>
                  <td className="mono">{c.fileName}</td>
                  <td>
                    {c.delta.slice(0, 3).map((d) => (
                      <span key={d.canonical} className="mr-2 small">
                        {controlLabel(d.canonical)} {fmtRaw(d.predictedRaw)} →{" "}
                        {fmtRaw(d.correctedRaw)}
                      </span>
                    ))}
                    {c.delta.length > 3 ? (
                      <span className="muted small">+{c.delta.length - 3} more</span>
                    ) : null}
                  </td>
                  <td className="num">{(c.correctionMagnitude * 100).toFixed(1)}%</td>
                  <td>
                    v{c.semanticVersion}{" "}
                    {c.includedInTrainingVersion ? (
                      <Badge tone="success" title={`used to train v${c.includedInTrainingVersion}`}>
                        trained
                      </Badge>
                    ) : (
                      <Badge tone="warning">pending</Badge>
                    )}
                  </td>
                  <td className="muted small">{formatDate(c.observedAt)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </Card>
      ) : null}
    </div>
  );
}
