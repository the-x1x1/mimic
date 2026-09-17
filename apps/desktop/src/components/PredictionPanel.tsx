import { Badge, Button, Card } from "@mimic/ui";
import { Check, Undo2, X } from "lucide-react";
import {
  CONFIDENCE_COMPONENT_LABELS,
  controlLabel,
  editMapping,
  predictedControlRows,
  type SessionPhoto,
} from "@mimic/contracts";
import { ApplyResultBadge, ConfidenceBadge } from "./ConfidenceBadge";

const controlByCanonical = new Map(editMapping.controls.map((c) => [c.canonical, c]));

function formatRaw(canonical: string, raw: unknown): string {
  if (typeof raw === "number") {
    if (canonical === "tone.exposure") return `${raw >= 0 ? "+" : ""}${raw.toFixed(2)} EV`;
    if (Number.isInteger(raw)) return `${raw >= 0 ? "+" : ""}${raw}`;
    return `${raw >= 0 ? "+" : ""}${raw.toFixed(2)}`;
  }
  if (raw === null || raw === undefined) return "—";
  return String(raw);
}

/**
 * Everything a reviewer needs for one photo: what will change, how confident the
 * model is and why, and what happened when it was applied. Actions are only
 * offered where the backend accepts them.
 */
export function PredictionPanel({
  photo,
  onReview,
  onApplyOne,
  busy,
  canApply,
  applyDisabledReason,
}: {
  photo: SessionPhoto;
  onReview: (status: "pending" | "reviewed" | "rejected") => void;
  onApplyOne?: () => void;
  busy?: boolean;
  canApply?: boolean;
  applyDisabledReason?: string;
}) {
  const p = photo.prediction;
  const a = photo.asset;
  const header = (
    <div className="stack gap-2">
      <div className="row gap-2 wrap">
        <strong>{a.fileName}</strong>
        <ConfidenceBadge prediction={p} />
        {photo.lastApply ? <ApplyResultBadge result={photo.lastApply.result} /> : null}
        {photo.burstId ? <Badge tone="info">burst</Badge> : null}
      </div>
      <div className="muted small">
        {[
          a.cameraModel,
          a.lens,
          a.iso ? `ISO ${a.iso}` : null,
          a.aperture ? `f/${a.aperture}` : null,
        ]
          .filter(Boolean)
          .join(" · ") || "no camera metadata"}
      </div>
    </div>
  );
  if (!p) {
    return (
      <Card>
        {header}
        <p className="muted mt-2">No prediction for this photo yet. Run Predict on the session.</p>
      </Card>
    );
  }
  const rows = predictedControlRows(p.predictedSettings);
  const comps = Object.entries(p.confidenceComponents).filter(
    ([k, v]) => typeof v === "number" && k in CONFIDENCE_COMPONENT_LABELS,
  ) as [string, number][];
  const reasons = p.rawModelOutput.reasons ?? [];
  const canDecide = p.status === "pending" || p.status === "reviewed";
  const mismatches =
    photo.lastApply?.result === "verify_failed"
      ? ((
          photo.lastApply.error as {
            mismatches?: { key: string; intended: unknown; observed: unknown }[];
          }
        )?.mismatches ?? [])
      : [];

  return (
    <Card
      actions={
        canDecide ? (
          <div className="row gap-2">
            {p.status !== "reviewed" ? (
              <Button
                size="sm"
                icon={<Check />}
                onClick={() => onReview("reviewed")}
                disabled={busy}
              >
                Looks right
              </Button>
            ) : null}
            <Button
              size="sm"
              variant="danger"
              icon={<X />}
              onClick={() => onReview("rejected")}
              disabled={busy}
            >
              Reject
            </Button>
            {onApplyOne ? (
              <Button
                size="sm"
                variant="primary"
                onClick={onApplyOne}
                disabled={busy || !canApply}
                title={
                  canApply
                    ? "Apply this photo through Lightroom with a before-snapshot"
                    : applyDisabledReason
                }
              >
                Apply this photo
              </Button>
            ) : null}
          </div>
        ) : p.status === "rejected" ? (
          <Button size="sm" icon={<Undo2 />} onClick={() => onReview("pending")} disabled={busy}>
            Un-reject
          </Button>
        ) : null
      }
    >
      <div className="pred-panel">
        {header}
        <section>
          <h4 className="pred-panel__h">Predicted changes</h4>
          {rows.length === 0 ? (
            <p className="muted small">The model produced no writable controls for this photo.</p>
          ) : (
            <table className="table pred-table">
              <thead>
                <tr>
                  <th>Control</th>
                  <th>Lightroom key</th>
                  <th className="num">Value</th>
                </tr>
              </thead>
              <tbody>
                {rows.map((r) => {
                  const lrKey = controlByCanonical.get(r.canonical)?.lightroomKeys[0] ?? "";
                  return (
                    <tr key={r.canonical}>
                      <td>{controlLabel(r.canonical)}</td>
                      <td className="mono muted">{lrKey}</td>
                      <td className="num mono">{formatRaw(r.canonical, r.raw)}</td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          )}
          {typeof p.rawModelOutput.consistencyShift === "number" &&
          p.rawModelOutput.consistencyShift > 0 ? (
            <p className="muted small">
              Scene consistency nudged colour/tone controls by up to{" "}
              {(p.rawModelOutput.consistencyShift * 100).toFixed(1)}% of range toward the group
              median. Exposure is never blended.
            </p>
          ) : null}
        </section>
        <section>
          <h4 className="pred-panel__h">Why this confidence</h4>
          <ul className="bar-list">
            {comps.map(([k, v]) => (
              <li key={k}>
                <span>{CONFIDENCE_COMPONENT_LABELS[k]}</span>
                <span className="bar-list__track">
                  <span
                    className="bar-list__fill"
                    style={{ width: `${Math.round(Math.max(0, Math.min(1, v)) * 100)}%` }}
                  />
                </span>
                <span className="bar-list__count">{Math.round(v * 100)}%</span>
              </li>
            ))}
          </ul>
          {reasons.length > 0 ? (
            <ul className="plain-list small">
              {reasons.map((r) => (
                <li key={r}>{r}</li>
              ))}
            </ul>
          ) : null}
          {p.nearestExamples.length > 0 ? (
            <p className="muted small">
              Based on {p.nearestExamples.length} nearest training photos from model v
              {p.modelVersionId.slice(0, 8)}…
            </p>
          ) : null}
        </section>
        {photo.lastApply ? (
          <section>
            <h4 className="pred-panel__h">Lightroom</h4>
            <p className="small">
              {photo.lastApply.result === "applied"
                ? `Applied and verified by read-back${photo.lastApply.lightroomSnapshotName ? ` · snapshot “${photo.lastApply.lightroomSnapshotName}”` : ""}.`
                : photo.lastApply.result === "verify_failed"
                  ? "Lightroom accepted the preset but read back different values. The before-snapshot is intact."
                  : photo.lastApply.result === "failed"
                    ? `Apply failed: ${(photo.lastApply.error as { message?: string })?.message ?? "unknown error"}`
                    : `Skipped: ${(photo.lastApply.error as { message?: string })?.message ?? ""}`}
            </p>
            {mismatches.length > 0 ? (
              <table className="table pred-table">
                <thead>
                  <tr>
                    <th>Key</th>
                    <th className="num">Sent</th>
                    <th className="num">Read back</th>
                  </tr>
                </thead>
                <tbody>
                  {mismatches.map((m) => (
                    <tr key={m.key}>
                      <td className="mono">{m.key}</td>
                      <td className="num mono">{String(m.intended)}</td>
                      <td className="num mono">{String(m.observed)}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            ) : null}
            {photo.lastApply.restoreResult ? (
              <p className="small muted">
                Restore: {photo.lastApply.restoreResult}
                {photo.lastApply.restoredAt ? ` at ${photo.lastApply.restoredAt}` : ""}
              </p>
            ) : null}
          </section>
        ) : null}
      </div>
    </Card>
  );
}
