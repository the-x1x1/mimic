import { Badge, Metric } from "@mimic/ui";
import type { DataQualityReport } from "@mimic/contracts";

const LEVEL_TONE = {
  insufficient: "danger",
  minimal: "warning",
  good: "success",
  strong: "success",
} as const;

export function DataQualityPanel({
  report,
  compact = false,
}: {
  report: DataQualityReport;
  compact?: boolean;
}) {
  const r = report;
  const topCameras = r.cameras.slice(0, compact ? 3 : 6);
  const coverage = r.assetsFound > 0 ? Math.round((r.validPairs / r.assetsFound) * 100) : 0;
  return (
    <div className="dq">
      <div className="dq__headline">
        <Badge tone={LEVEL_TONE[r.recommendation.level]}>{r.recommendation.level}</Badge>
        <div>
          <div className="dq__title">{r.recommendation.headline}</div>
          <div className="muted small">{r.recommendation.detail}</div>
        </div>
      </div>
      <div className="metric-grid">
        <Metric label="Photos found" value={r.assetsFound.toLocaleString()} />
        <Metric
          label="Edit pairs"
          value={r.validPairs.toLocaleString()}
          hint={`${coverage}% of photos have edits`}
          tone={r.validPairs === 0 ? "bad" : undefined}
        />
        <Metric
          label="Missing edits"
          value={r.missingEdits.toLocaleString()}
          tone={r.missingEdits > 0 && r.validPairs === 0 ? "warn" : undefined}
        />
        <Metric
          label="Analyzed"
          value={r.featuresComputed.toLocaleString()}
          hint="visual features computed"
        />
        {!compact ? (
          <Metric
            label="Lightroom-connected"
            value={r.lightroomConnectedPairs.toLocaleString()}
            hint="via plugin"
          />
        ) : null}
        {!compact ? (
          <Metric label="Sidecar-only" value={r.sidecarOnlyPairs.toLocaleString()} hint="via XMP" />
        ) : null}
        {!compact ? (
          <Metric
            label="ACR sidecars"
            value={r.acrHeavyEditCount.toLocaleString()}
            hint="heavy edits unreadable offline"
            tone={r.acrHeavyEditCount > 0 ? "warn" : undefined}
          />
        ) : null}
        {!compact ? (
          <Metric
            label="Local edits"
            value={r.localEditCount.toLocaleString()}
            hint="masks observed, not learned"
          />
        ) : null}
      </div>
      {topCameras.length > 0 ? (
        <div className="dq__section">
          <div className="dq__label">Cameras</div>
          <ul className="bar-list">
            {topCameras.map((c) => (
              <li key={c.label}>
                <span className="bar-list__label">{c.label}</span>
                <span className="bar-list__track">
                  <span
                    className="bar-list__fill"
                    style={{
                      width: `${Math.max(2, (c.count / Math.max(1, r.assetsFound)) * 100)}%`,
                    }}
                  />
                </span>
                <span className="bar-list__count">{c.count.toLocaleString()}</span>
              </li>
            ))}
          </ul>
        </div>
      ) : null}
      {!compact && r.captureDays.length > 0 ? (
        <div className="dq__section">
          <div className="dq__label">Shoot days</div>
          <div className="muted small">
            {r.captureDays.length} day{r.captureDays.length === 1 ? "" : "s"} ·{" "}
            {r.captureDays[0]?.label} → {r.captureDays[r.captureDays.length - 1]?.label}
          </div>
        </div>
      ) : null}
      {r.warnings.length > 0 ? (
        <ul className="dq__warnings">
          {r.warnings.map((w) => (
            <li key={w}>{w}</li>
          ))}
        </ul>
      ) : null}
    </div>
  );
}
