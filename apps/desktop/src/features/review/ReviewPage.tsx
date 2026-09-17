import { ClipboardCheck } from "lucide-react";
import { EmptyState } from "@mimic/ui";
import { PageHeader } from "@/components/PageHeader";

export function ReviewPage() {
  return (
    <>
      <PageHeader
        title="Review"
        subtitle="Only the photos that need your attention: low confidence, unfamiliar scenes, apply failures."
      />
      <EmptyState
        icon={<ClipboardCheck />}
        title="Nothing to review"
        body="Review appears once a session has predictions (0.3.0). Confidence thresholds are configurable in Settings › General when that ships."
      />
    </>
  );
}
