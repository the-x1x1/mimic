import { Badge } from "@mimic/ui";
import {
  confidenceBand,
  DEFAULT_HIGH_THRESHOLD,
  DEFAULT_REVIEW_THRESHOLD,
  type Prediction,
} from "@mimic/contracts";

/**
 * One glance: how sure the Style Brain is about this photo. The number is the
 * calibrated confidence written by the engine; "unfamiliar" means the photo sits
 * outside the training distribution and the prediction is a guess.
 */
export function ConfidenceBadge({
  prediction,
  threshold = DEFAULT_REVIEW_THRESHOLD,
  high = DEFAULT_HIGH_THRESHOLD,
}: {
  prediction: Prediction | null;
  threshold?: number;
  high?: number;
}) {
  if (!prediction) return <Badge tone="neutral">not predicted</Badge>;
  if (prediction.status === "rejected") return <Badge tone="neutral">rejected</Badge>;
  const ood = prediction.rawModelOutput.ood === true;
  const band = confidenceBand(prediction.confidence, threshold, high);
  const pctText = `${Math.round(prediction.confidence * 100)}%`;
  if (ood)
    return (
      <Badge tone="danger" title="Outside the training distribution; review before applying">
        {pctText} · unfamiliar
      </Badge>
    );
  const tone = band === "high" ? "success" : band === "medium" ? "warning" : "danger";
  return (
    <Badge
      tone={tone}
      title={`confidence ${pctText}; review threshold ${Math.round(threshold * 100)}%`}
    >
      {pctText}
      {prediction.status === "applied"
        ? " · applied"
        : prediction.status === "reviewed"
          ? " · reviewed"
          : ""}
    </Badge>
  );
}

export function ApplyResultBadge({ result }: { result: string | null | undefined }) {
  if (!result) return null;
  switch (result) {
    case "applied":
      return <Badge tone="success">applied · verified</Badge>;
    case "verify_failed":
      return <Badge tone="danger">read-back mismatch</Badge>;
    case "failed":
      return <Badge tone="danger">apply failed</Badge>;
    case "skipped":
      return <Badge tone="neutral">skipped</Badge>;
    default:
      return <Badge>{result}</Badge>;
  }
}
