import { useEffect, useMemo, useState } from "react";
import { Link, useSearchParams } from "react-router-dom";
import { Badge, Button, EmptyState } from "@mimic/ui";
import { ChevronLeft, ChevronRight, ClipboardCheck, ImageOff } from "lucide-react";
import { DEFAULT_REVIEW_THRESHOLD, needsAttention } from "@mimic/contracts";
import { PageHeader } from "@/components/PageHeader";
import { ConfidenceBadge } from "@/components/ConfidenceBadge";
import { PredictionPanel } from "@/components/PredictionPanel";
import { useLightroomStatus } from "@/hooks/useLightroom";
import {
  useApplyPreflight,
  useApplySession,
  useSessionDetail,
  useSessionPhotos,
  useSessions,
  useSetPredictionReview,
} from "@/hooks/useSessions";
import { useSettings } from "@/hooks/useSystem";
import { previewUrl } from "@/lib/ipc";
import { ConfirmApplyDialog } from "../sessions/ConfirmApplyDialog";
import { reviewQueue, type Filter } from "./reviewQueue";

export function ReviewPage() {
  const [params, setParams] = useSearchParams();
  const sessions = useSessions();
  const sessionId =
    params.get("session") ?? sessions.data?.find((s) => s.status !== "new")?.id ?? null;
  const detail = useSessionDetail(sessionId);
  const photos = useSessionPhotos(sessionId);
  const lr = useLightroomStatus();
  const settings = useSettings();
  const threshold = settings.data?.["review.mediumThreshold"] ?? DEFAULT_REVIEW_THRESHOLD;
  const review = useSetPredictionReview(sessionId ?? "");
  const apply = useApplySession();
  const [filter, setFilter] = useState<Filter>("attention");
  const [index, setIndex] = useState(0);
  const [confirmIds, setConfirmIds] = useState<string[] | null>(null);
  const preflight = useApplyPreflight(sessionId, confirmIds ?? undefined, confirmIds !== null);
  const lrConnected = lr.data?.bridge.connected ?? false;

  const queue = useMemo(
    () => reviewQueue(photos.data ?? [], filter, threshold),
    [photos.data, filter, threshold],
  );
  useEffect(() => {
    if (index >= queue.length) setIndex(Math.max(0, queue.length - 1));
  }, [queue.length, index]);
  const current = queue[index] ?? null;
  const src = previewUrl(current?.previewPath);
  const attention = (photos.data ?? []).filter((p) => needsAttention(p, threshold)).length;

  const selectSession = (id: string) => {
    params.set("session", id);
    setParams(params, { replace: true });
    setIndex(0);
  };

  const decide = (status: "pending" | "reviewed" | "rejected") => {
    if (!current?.prediction) return;
    review.mutate(
      { predictionId: current.prediction.id, status },
      { onSuccess: () => setIndex((i) => Math.min(i + 1, Math.max(0, queue.length - 1))) },
    );
  };

  return (
    <>
      <PageHeader
        title="Review"
        subtitle={`Only what needs your eyes: predictions under ${Math.round(threshold * 100)}% confidence, unfamiliar scenes, and photos whose apply did not verify.`}
        actions={
          sessions.data && sessions.data.length > 0 ? (
            <select
              className="input input--narrow"
              value={sessionId ?? ""}
              onChange={(e) => selectSession(e.target.value)}
              aria-label="Session"
            >
              {sessions.data.map((s) => (
                <option key={s.id} value={s.id}>
                  {s.name}
                </option>
              ))}
            </select>
          ) : null
        }
      />
      {!sessionId ? (
        <EmptyState
          icon={<ClipboardCheck />}
          title="Nothing to review"
          body="Create a session and run Predict; photos that need attention show up here."
          primary={
            <Link to="/sessions" className="ui-btn ui-btn--primary ui-btn--md">
              Go to Sessions
            </Link>
          }
        />
      ) : (
        <>
          <div className="row gap-2 wrap mt-2" role="tablist" aria-label="Review filter">
            <button
              className={filter === "attention" ? "seg seg--on" : "seg"}
              onClick={() => setFilter("attention")}
            >
              Needs attention ({attention})
            </button>
            <button
              className={filter === "pending" ? "seg seg--on" : "seg"}
              onClick={() => setFilter("pending")}
            >
              All pending (
              {(detail.data?.predictionCounts.pending ?? 0) +
                (detail.data?.predictionCounts.reviewed ?? 0)}
              )
            </button>
            <button
              className={filter === "all" ? "seg seg--on" : "seg"}
              onClick={() => setFilter("all")}
            >
              Every prediction
            </button>
            {detail.data && !lrConnected ? (
              <Badge tone="neutral" title="Review works offline; Apply needs Lightroom">
                Lightroom offline
              </Badge>
            ) : null}
          </div>
          {queue.length === 0 ? (
            <EmptyState
              icon={<ClipboardCheck />}
              title={filter === "attention" ? "Nothing needs attention" : "No predictions here"}
              body={
                filter === "attention"
                  ? "Every prediction in this session is above the review threshold and applied or awaiting apply."
                  : "Run Predict on the session to populate the queue."
              }
            />
          ) : current ? (
            <div className="review-layout mt-3">
              <div className="review-preview">
                {src ? <img src={src} alt={current.asset.fileName} /> : <ImageOff size={32} />}
                <div className="review-nav">
                  <Button
                    size="sm"
                    icon={<ChevronLeft />}
                    onClick={() => setIndex((i) => Math.max(0, i - 1))}
                    disabled={index === 0}
                    aria-label="Previous"
                  />
                  <span className="muted small">
                    {index + 1} / {queue.length}
                  </span>
                  <Button
                    size="sm"
                    icon={<ChevronRight />}
                    onClick={() => setIndex((i) => Math.min(queue.length - 1, i + 1))}
                    disabled={index >= queue.length - 1}
                    aria-label="Next"
                  />
                </div>
                <div className="filmstrip" role="list">
                  {queue.map((p, i) => {
                    const thumb = previewUrl(p.previewPath);
                    return (
                      <button
                        key={p.asset.id}
                        role="listitem"
                        className={
                          i === index ? "filmstrip__item filmstrip__item--on" : "filmstrip__item"
                        }
                        onClick={() => setIndex(i)}
                        title={p.asset.fileName}
                      >
                        {thumb ? <img src={thumb} alt="" loading="lazy" /> : <ImageOff size={14} />}
                        <ConfidenceBadge prediction={p.prediction} threshold={threshold} />
                      </button>
                    );
                  })}
                </div>
              </div>
              <div className="review-side">
                <PredictionPanel
                  photo={current}
                  busy={review.isPending}
                  onReview={decide}
                  canApply={lrConnected}
                  applyDisabledReason="Lightroom is not connected."
                  onApplyOne={
                    current.prediction ? () => setConfirmIds([current.prediction!.id]) : undefined
                  }
                />
                <p className="muted small mt-2">
                  “Looks right” marks the prediction reviewed; it is still applied only when you
                  choose Apply. Rejected photos are never applied.
                </p>
              </div>
            </div>
          ) : null}
        </>
      )}
      <ConfirmApplyDialog
        open={confirmIds !== null}
        onOpenChange={(o) => !o && setConfirmIds(null)}
        preflight={preflight.data}
        photoCount={confirmIds?.length ?? 0}
        onConfirm={() =>
          sessionId &&
          confirmIds &&
          apply.mutate(
            { sessionId, predictionIds: confirmIds },
            { onSuccess: () => setConfirmIds(null) },
          )
        }
        busy={apply.isPending}
        scopeLabel={current?.asset.fileName ?? ""}
      />
    </>
  );
}
