import { needsAttention, type SessionPhoto } from "@mimic/contracts";

export type Filter = "attention" | "pending" | "all";

/** Pure: which photos the queue shows for a filter. Exported for tests. */
export function reviewQueue(
  photos: SessionPhoto[],
  filter: Filter,
  threshold: number,
): SessionPhoto[] {
  switch (filter) {
    case "attention":
      return photos.filter((p) => needsAttention(p, threshold));
    case "pending":
      return photos.filter(
        (p) =>
          p.prediction && (p.prediction.status === "pending" || p.prediction.status === "reviewed"),
      );
    default:
      return photos.filter((p) => p.prediction !== null);
  }
}
