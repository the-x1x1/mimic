import { Link, useNavigate } from "react-router-dom";
import { Button, Card, EmptyState, Metric } from "@mimic/ui";
import { ArrowRight, Images, Layers, Sparkles } from "lucide-react";
import { PageHeader } from "@/components/PageHeader";
import { useSystemStatus } from "@/hooks/useSystem";
import { useStyles } from "@/hooks/useStyles";
import { useLibraries } from "@/hooks/useLibraries";
import { formatRelative } from "@/lib/format";
import { JOB_LABELS, primaryError } from "@mimic/contracts";

export function HomePage() {
  const navigate = useNavigate();
  const system = useSystemStatus();
  const styles = useStyles();
  const libraries = useLibraries();
  const style = styles.data?.[0];
  const totalPairs = (libraries.data ?? []).reduce((n, l) => n + l.validPairCount, 0);

  if (styles.isSuccess && styles.data.length === 0) {
    return (
      <>
        <PageHeader title="Home" />
        <EmptyState
          icon={<Sparkles />}
          title="Mimic needs examples of your editing before it can edit a new shoot."
          body="Create a Style from a folder of RAW files with Lightroom sidecars, or connect Lightroom Classic and capture develop settings directly."
          primary={
            <Button variant="primary" size="lg" onClick={() => navigate("/styles?new=1")}>
              Create a Style
            </Button>
          }
          secondary={
            <Button size="lg" onClick={() => navigate("/settings?section=lightroom")}>
              Plugin setup
            </Button>
          }
        />
      </>
    );
  }

  return (
    <>
      <PageHeader title="Home" />
      <div className="home-grid">
        <div className="stack gap-4">
          <div className="action-row">
            <button
              className="action-card action-card--primary"
              onClick={() => navigate("/sessions")}
            >
              <Images size={22} />
              <div>
                <div className="action-card__title">Edit a New Session</div>
                <div className="action-card__sub">
                  Ingest a shoot, group scenes, predict in your Style, apply to Lightroom with
                  snapshots and read-back.
                </div>
              </div>
              <ArrowRight size={18} />
            </button>
            <button className="action-card" onClick={() => navigate("/styles")}>
              <Layers size={22} />
              <div>
                <div className="action-card__title">Train / Improve a Style</div>
                <div className="action-card__sub">
                  Add training data, check dataset quality and train immutable versions.
                </div>
              </div>
              <ArrowRight size={18} />
            </button>
          </div>

          <Card
            title="Active Style"
            actions={style ? <Link to={`/styles/${style.id}`}>Open</Link> : null}
          >
            {style ? (
              <div className="metric-grid">
                <Metric
                  label="Style"
                  value={style.name}
                  hint={style.cameras.slice(0, 2).join(", ") || "no cameras yet"}
                />
                <Metric
                  label="Model version"
                  value={style.activeVersion?.semanticVersion ?? "none"}
                  hint={style.activeVersion ? "active" : "not trained yet"}
                />
                <Metric
                  label="Historical examples"
                  value={style.trainingExamples.toLocaleString()}
                />
                <Metric
                  label="Holdout error"
                  value={
                    style.activeVersion
                      ? (primaryError(style.activeVersion.metrics)?.toFixed(3) ?? "—")
                      : "—"
                  }
                  hint={
                    style.activeVersion
                      ? "nMAE on unseen shoots (lower is better)"
                      : "train a version to see quality"
                  }
                />
                <Metric
                  label="No-Touch Rate"
                  value="—"
                  hint="needs corrections sync to know which applied photos you left untouched (0.4.0)"
                />
              </div>
            ) : (
              <p className="muted">Loading…</p>
            )}
          </Card>

          <Card title="Recent activity">
            {(system.data?.activeJobs.length ?? 0) > 0 ? (
              <ul className="plain-list">
                {system.data!.activeJobs.map((j) => (
                  <li key={j.id}>
                    {JOB_LABELS[j.type] ?? j.type} · {j.phase ?? j.status} ·{" "}
                    {j.progressTotal > 0 ? `${j.progressCurrent}/${j.progressTotal}` : ""}
                  </li>
                ))}
              </ul>
            ) : (
              <p className="muted">
                Nothing running.{" "}
                {totalPairs > 0
                  ? `${totalPairs.toLocaleString()} edited examples ingested so far.`
                  : ""}
              </p>
            )}
          </Card>
        </div>

        <aside className="stack gap-3">
          <Card title="System">
            <ul className="status-list">
              <li>
                <span
                  className={`dot ${system.data?.lightroom.connected ? "dot--good" : "dot--off"}`}
                />
                Lightroom {system.data?.lightroom.connected ? "connected" : "not connected"}
              </li>
              <li>
                <span
                  className={`dot ${system.data?.engine.state === "ready" ? "dot--good" : system.data?.engine.state === "starting" ? "dot--warn" : "dot--bad"}`}
                />
                Engine {system.data?.engine.state ?? "…"}
                {system.data?.engine.accelerator && system.data.engine.accelerator !== "cpu"
                  ? ` (${system.data.engine.accelerator})`
                  : ""}
              </li>
              <li>
                <span
                  className={`dot ${system.data?.update.stagedVersion ? "dot--warn" : "dot--off"}`}
                />
                {system.data?.update.stagedVersion
                  ? `Update ${system.data.update.stagedVersion} staged`
                  : `Updates checked ${formatRelative(system.data?.update.lastCheckedAt)}`}
              </li>
            </ul>
          </Card>
          <Card title="Everything stays on this computer">
            <p className="muted small">
              Analysis, training data and history live in your local app data folder. No photo,
              sidecar or model leaves this machine.
            </p>
          </Card>
        </aside>
      </div>
    </>
  );
}
