import { useMemo, useState } from "react";
import { Link, useNavigate, useParams } from "react-router-dom";
import { Badge, Button, Card, EmptyState, InlineError, Metric, ProgressBar } from "@mimic/ui";
import { ImageOff, Layers3, RefreshCw, RotateCcw, Sparkles, Trash2, Upload } from "lucide-react";
import { JOB_LABELS, needsAttention, type SessionPhoto } from "@mimic/contracts";
import { PageHeader } from "@/components/PageHeader";
import { ApplyResultBadge, ConfidenceBadge } from "@/components/ConfidenceBadge";
import { PredictionPanel } from "@/components/PredictionPanel";
import { useJobs } from "@/hooks/useJobs";
import { useLightroomStatus } from "@/hooks/useLightroom";
import {
  useApplyPreflight,
  useApplySession,
  useDeleteSession,
  useGroupSession,
  usePredictSession,
  useRestoreBatch,
  useSessionDetail,
  useSessionPhotos,
  useSetPredictionReview,
  useSetSessionStyle,
  useSyncCorrections,
} from "@/hooks/useSessions";
import { useStyles } from "@/hooks/useStyles";
import { previewUrl } from "@/lib/ipc";
import { formatDate } from "@/lib/format";
import { ConfirmApplyDialog } from "./ConfirmApplyDialog";
import { SESSION_STATUS_LABEL, sessionStatusTone } from "./sessionStatus";

const SESSION_JOBS = [
  "ingest_session",
  "group_session",
  "predict_session",
  "apply_session",
  "restore_batch",
  "sync_corrections",
];

export function SessionDetailPage() {
  const { sessionId = "" } = useParams();
  const navigate = useNavigate();
  const detail = useSessionDetail(sessionId);
  const photos = useSessionPhotos(sessionId);
  const styles = useStyles();
  const lr = useLightroomStatus();
  const jobs = useJobs(true);
  const group = useGroupSession();
  const predict = usePredictSession();
  const apply = useApplySession();
  const restore = useRestoreBatch(sessionId);
  const review = useSetPredictionReview(sessionId);
  const setStyle = useSetSessionStyle();
  const del = useDeleteSession();
  const sync = useSyncCorrections();
  const [clusterFilter, setClusterFilter] = useState<string | "all" | "attention">("all");
  const [selected, setSelected] = useState<string | null>(null);
  const [confirm, setConfirm] = useState<{ ids?: string[]; label: string } | null>(null);
  const preflight = useApplyPreflight(sessionId, confirm?.ids, confirm !== null);

  const activeJob = jobs.data?.find(
    (j) =>
      SESSION_JOBS.includes(j.type) &&
      ((j.payload as { sessionId?: string } | null)?.sessionId === sessionId ||
        detail.data?.batches.some(
          (b) => b.id === (j.payload as { applyBatchId?: string } | null)?.applyBatchId,
        )),
  );

  const visible = useMemo(() => {
    const all = photos.data ?? [];
    if (clusterFilter === "all") return all;
    if (clusterFilter === "attention") return all.filter((p) => needsAttention(p));
    return all.filter((p) => p.clusterId === clusterFilter);
  }, [photos.data, clusterFilter]);

  if (detail.isError)
    return <InlineError title="Session not found">{(detail.error as Error).message}</InlineError>;
  if (!detail.data) return <p className="muted">Loading…</p>;
  const {
    session,
    clusters,
    batches,
    correctionSyncs,
    predictionCounts,
    grouped,
    photoCount,
    photosWithFeatures,
  } = detail.data;
  const appliedVerified = batches.reduce((n, b) => n + b.appliedCount, 0);
  const trainedStyles = (styles.data ?? []).filter((s) => s.activeVersion);
  const style = (styles.data ?? []).find((s) => s.id === session.activeStyleProfileId);
  const lrConnected = lr.data?.bridge.connected ?? false;
  const pending = (predictionCounts.pending ?? 0) + (predictionCounts.reviewed ?? 0);
  const attentionCount = (photos.data ?? []).filter((p) => needsAttention(p)).length;
  const selectedPhoto = (photos.data ?? []).find((p) => p.asset.id === selected) ?? null;
  const busy = !!activeJob;
  const canGroup = photoCount > 0 && photosWithFeatures > 0 && !busy;
  const canPredict = !!style?.activeVersion && photoCount > 0 && !busy;
  const predictReason = !style
    ? "Choose a trained Style first."
    : !style.activeVersion
      ? `${style.name} has no active trained version.`
      : photoCount === 0
        ? "No photos ingested."
        : busy
          ? "Wait for the running job."
          : undefined;
  const canApply = pending > 0 && lrConnected && !busy;
  const canSync = appliedVerified > 0 && lrConnected && !busy;
  const syncReason = !lrConnected
    ? "Lightroom is not connected."
    : appliedVerified === 0
      ? "Nothing applied yet: sync compares your final edits with what Mimic applied."
      : busy
        ? "Wait for the running job."
        : undefined;
  const applyReason = !lrConnected
    ? "Lightroom is not connected."
    : pending === 0
      ? "Nothing pending: run Predict, or every prediction is applied/rejected."
      : busy
        ? "Wait for the running job."
        : undefined;

  const runApply = () => {
    if (!confirm) return;
    apply.mutate({ sessionId, predictionIds: confirm.ids }, { onSuccess: () => setConfirm(null) });
  };

  return (
    <>
      <PageHeader
        title={session.name}
        subtitle={
          <span className="row gap-2 wrap">
            <Badge tone={sessionStatusTone(session.status)}>
              {SESSION_STATUS_LABEL[session.status] ?? session.status}
            </Badge>
            <span className="muted">
              {photoCount} photos · {photosWithFeatures} analyzed
              {session.capturedStart ? ` · ${formatDate(session.capturedStart)}` : ""}
            </span>
          </span>
        }
        actions={
          <>
            <select
              className="input input--narrow"
              value={session.activeStyleProfileId ?? ""}
              onChange={(e) => setStyle.mutate({ sessionId, styleId: e.target.value || null })}
              aria-label="Style"
              disabled={busy}
            >
              <option value="">No Style</option>
              {trainedStyles.map((s) => (
                <option key={s.id} value={s.id}>
                  {s.name} · v{s.activeVersion?.semanticVersion}
                </option>
              ))}
            </select>
            <Button
              icon={<Layers3 />}
              onClick={() => group.mutate(sessionId)}
              disabled={!canGroup}
              loading={group.isPending}
              title={
                canGroup
                  ? "Split the shoot into time blocks and visual scene groups"
                  : "Ingest photos first"
              }
            >
              {grouped ? "Re-group" : "Analyze scenes"}
            </Button>
            <Button
              icon={<Sparkles />}
              onClick={() => predict.mutate({ sessionId })}
              disabled={!canPredict}
              loading={predict.isPending}
              title={predictReason ?? "Predict edits for every photo with the active Style version"}
            >
              Predict
            </Button>
            <Button
              variant="primary"
              icon={<Upload />}
              onClick={() => setConfirm({ label: "all pending predictions" })}
              disabled={!canApply}
              title={applyReason ?? "Apply pending predictions to Lightroom"}
            >
              Apply to Lightroom
            </Button>
            <Button
              icon={<RefreshCw />}
              onClick={() => sync.mutate(sessionId)}
              disabled={!canSync}
              loading={sync.isPending}
              title={
                syncReason ??
                "Read the applied photos back from Lightroom; what you changed becomes a correction"
              }
            >
              Sync corrections
            </Button>
            <Button
              variant="danger"
              icon={<Trash2 />}
              aria-label="Delete session"
              disabled={busy}
              onClick={() => {
                if (
                  window.confirm(
                    `Delete session “${session.name}”? Applied edits in Lightroom are not changed.`,
                  )
                )
                  del.mutate(sessionId, { onSuccess: () => navigate("/sessions") });
              }}
            />
          </>
        }
      />

      {activeJob ? (
        <Card title={JOB_LABELS[activeJob.type] ?? activeJob.type}>
          <ProgressBar
            current={activeJob.progressCurrent}
            total={activeJob.progressTotal}
            label={activeJob.status === "queued" ? "queued" : (activeJob.phase ?? "working")}
          />
          {activeJob.type === "apply_session" ? (
            <p className="muted small mt-2">
              Stopping takes effect between batches; photos already written keep their
              before-snapshot.
            </p>
          ) : null}
        </Card>
      ) : null}
      {session.status === "ingest_failed" ? (
        <InlineError title="Ingest failed">
          Check the job in Settings › Diagnostics. Common causes: the folder moved, or Lightroom
          disconnected mid-way.
        </InlineError>
      ) : null}

      <div className="metric-grid mt-3">
        <Metric
          label="Scene groups"
          value={clusters.length || "—"}
          hint={grouped ? undefined : "not analyzed yet"}
        />
        <Metric
          label="Pending"
          value={pending}
          hint={
            predictionCounts.applied
              ? `${predictionCounts.applied} applied`
              : "predictions awaiting apply"
          }
        />
        <Metric
          label="Needs attention"
          value={attentionCount}
          tone={attentionCount > 0 ? "warn" : "neutral"}
          hint="low confidence, unfamiliar, or failed"
        />
        <Metric
          label="Lightroom"
          value={lrConnected ? "connected" : "offline"}
          tone={lrConnected ? "good" : "warn"}
          hint={style ? `Style: ${style.name}` : "no Style chosen"}
        />
      </div>

      {batches.length > 0 ? (
        <Card title="Apply history" className="mt-3">
          <table className="table">
            <thead>
              <tr>
                <th>Started</th>
                <th>Status</th>
                <th className="num">Applied</th>
                <th className="num">Failed</th>
                <th></th>
              </tr>
            </thead>
            <tbody>
              {batches.map((b) => (
                <tr key={b.id}>
                  <td>{formatDate(b.startedAt)}</td>
                  <td>
                    <Badge
                      tone={
                        b.status === "completed"
                          ? "success"
                          : b.status === "running"
                            ? "info"
                            : b.status === "completed_with_failures"
                              ? "warning"
                              : "danger"
                      }
                    >
                      {b.status.replaceAll("_", " ")}
                    </Badge>
                  </td>
                  <td className="num">{b.appliedCount}</td>
                  <td className="num">{b.failedCount}</td>
                  <td className="num">
                    {b.rollbackAvailable && b.status !== "running" ? (
                      <Button
                        size="sm"
                        icon={<RotateCcw />}
                        disabled={!lrConnected || busy}
                        title={
                          lrConnected
                            ? "Write the recorded before-values back and verify"
                            : "Connect Lightroom to restore"
                        }
                        onClick={() => {
                          if (
                            window.confirm(
                              "Restore every photo in this batch to its recorded before-values?",
                            )
                          )
                            restore.mutate(b.id);
                        }}
                      >
                        Restore
                      </Button>
                    ) : (
                      <span className="muted small">
                        {b.rollbackAvailable ? "" : "restored / nothing to restore"}
                      </span>
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </Card>
      ) : null}

      {correctionSyncs.length > 0 ? (
        <Card title="Corrections synced" className="mt-3">
          <table className="table">
            <thead>
              <tr>
                <th>When</th>
                <th className="num">Checked</th>
                <th className="num">Untouched</th>
                <th className="num">Corrected</th>
                <th className="num">No-Touch</th>
              </tr>
            </thead>
            <tbody>
              {correctionSyncs.map((c) => (
                <tr key={c.id}>
                  <td>{formatDate(c.syncedAt)}</td>
                  <td className="num">{c.checkedCount}</td>
                  <td className="num">{c.untouchedCount}</td>
                  <td className="num">{c.correctedCount}</td>
                  <td className="num">
                    {c.checkedCount > 0
                      ? `${Math.round((c.untouchedCount / c.checkedCount) * 100)}%`
                      : "—"}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
          <p className="muted small mt-2">
            Corrections feed the Style's next training run; see the Style's Corrections tab.
          </p>
        </Card>
      ) : null}

      <div className="row gap-2 wrap mt-4" role="tablist" aria-label="Filter photos">
        <button
          className={clusterFilter === "all" ? "seg seg--on" : "seg"}
          onClick={() => setClusterFilter("all")}
        >
          All ({photoCount})
        </button>
        <button
          className={clusterFilter === "attention" ? "seg seg--on" : "seg"}
          onClick={() => setClusterFilter("attention")}
        >
          Needs attention ({attentionCount})
        </button>
        {clusters.map((c) => (
          <button
            key={c.id}
            className={clusterFilter === c.id ? "seg seg--on" : "seg"}
            onClick={() => setClusterFilter(c.id)}
            title={`time block ${String(c.featureSummary.timeBlock ?? "?")}`}
          >
            {c.label} ({c.assetCount})
          </button>
        ))}
        <Link to={`/review?session=${sessionId}`} className="ml-2 small">
          Open in Review →
        </Link>
      </div>

      {photoCount === 0 ? (
        <EmptyState
          icon={<ImageOff />}
          title={session.status === "ingesting" ? "Ingesting…" : "No photos in this session"}
          body={
            session.status === "ingesting"
              ? "Scanning files, reading metadata and computing visual features."
              : "The folder or selection had no supported RAW/JPEG/TIFF files."
          }
        />
      ) : (
        <div className="session-layout mt-3">
          <div className="photo-grid" role="list">
            {visible.map((p) => (
              <SessionTile
                key={p.asset.id}
                photo={p}
                selected={p.asset.id === selected}
                onSelect={() => setSelected(p.asset.id === selected ? null : p.asset.id)}
              />
            ))}
          </div>
          {selectedPhoto ? (
            <div className="session-side">
              <PredictionPanel
                photo={selectedPhoto}
                busy={review.isPending || busy}
                onReview={(status) =>
                  selectedPhoto.prediction &&
                  review.mutate({ predictionId: selectedPhoto.prediction.id, status })
                }
                canApply={lrConnected && !busy}
                applyDisabledReason={applyReason}
                onApplyOne={
                  selectedPhoto.prediction
                    ? () =>
                        setConfirm({
                          ids: [selectedPhoto.prediction!.id],
                          label: selectedPhoto.asset.fileName,
                        })
                    : undefined
                }
              />
            </div>
          ) : null}
        </div>
      )}

      <ConfirmApplyDialog
        open={confirm !== null}
        onOpenChange={(o) => !o && setConfirm(null)}
        preflight={preflight.data}
        photoCount={preflight.data?.candidateCount ?? confirm?.ids?.length ?? pending}
        onConfirm={runApply}
        busy={apply.isPending}
        scopeLabel={confirm?.label ?? ""}
      />
    </>
  );
}

export function SessionTile({
  photo,
  selected,
  onSelect,
}: {
  photo: SessionPhoto;
  selected: boolean;
  onSelect: () => void;
}) {
  const src = previewUrl(photo.previewPath);
  return (
    <button
      className={selected ? "tile tile--selected" : "tile"}
      role="listitem"
      aria-pressed={selected}
      onClick={onSelect}
      title={photo.asset.sourcePath}
    >
      <div className="tile__image">
        {src ? (
          <img src={src} alt="" loading="lazy" decoding="async" />
        ) : (
          <div className="tile__placeholder">
            <ImageOff size={18} />
          </div>
        )}
      </div>
      <div className="tile__meta">
        <span className="tile__name">{photo.asset.fileName}</span>
        {photo.lastApply && photo.lastApply.result !== "applied" ? (
          <ApplyResultBadge result={photo.lastApply.result} />
        ) : (
          <ConfidenceBadge prediction={photo.prediction} />
        )}
      </div>
    </button>
  );
}
