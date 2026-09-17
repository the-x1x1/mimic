export function sessionStatusTone(
  status: string,
): "neutral" | "success" | "warning" | "danger" | "info" {
  switch (status) {
    case "applied":
      return "success";
    case "predicted":
    case "grouped":
      return "info";
    case "ingesting":
    case "applying":
      return "warning";
    case "ingest_failed":
      return "danger";
    default:
      return "neutral";
  }
}

export const SESSION_STATUS_LABEL: Record<string, string> = {
  new: "new",
  ingesting: "ingesting…",
  ingest_failed: "ingest failed",
  empty: "no photos found",
  ingested: "ready to analyze",
  grouped: "grouped",
  predicted: "predicted",
  applying: "applying…",
  applied: "applied",
};
