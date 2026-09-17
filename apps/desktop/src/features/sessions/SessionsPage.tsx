import { Images } from "lucide-react";
import { EmptyState } from "@mimic/ui";
import { PageHeader } from "@/components/PageHeader";

export function SessionsPage() {
  return (
    <>
      <PageHeader
        title="Sessions"
        subtitle="A session is a new shoot Mimic analyzes as scene groups, predicts, and applies to Lightroom."
      />
      <EmptyState
        icon={<Images />}
        title="Sessions are not available in this build"
        body="This release (0.1) covers ingest and dataset quality. Session creation, scene grouping, prediction with confidence and Lightroom apply ship in 0.3.0. Nothing here pretends otherwise."
      />
    </>
  );
}
