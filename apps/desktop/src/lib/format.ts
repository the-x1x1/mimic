export function formatBytes(n: number): string {
  if (!Number.isFinite(n) || n <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB"];
  const i = Math.min(units.length - 1, Math.floor(Math.log(n) / Math.log(1024)));
  return `${(n / 1024 ** i).toFixed(i === 0 ? 0 : 1)} ${units[i]}`;
}

export function formatRelative(iso: string | null | undefined, now: Date = new Date()): string {
  if (!iso) return "never";
  const then = new Date(iso).getTime();
  if (Number.isNaN(then)) return iso;
  const diff = Math.max(0, now.getTime() - then);
  const s = Math.round(diff / 1000);
  if (s < 45) return "just now";
  const m = Math.round(s / 60);
  if (m < 60) return `${m} min ago`;
  const h = Math.round(m / 60);
  if (h < 24) return `${h} h ago`;
  const d = Math.round(h / 24);
  if (d < 30) return `${d} d ago`;
  return new Date(iso).toLocaleDateString();
}

export function formatDate(iso: string | null | undefined): string {
  if (!iso) return "—";
  const d = new Date(iso);
  return Number.isNaN(d.getTime())
    ? iso
    : d.toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" });
}

/** A count with its noun, pluralized. `countOf(1, "message")` → "1 message". */
export function countOf(n: number, noun: string, plural = `${noun}s`): string {
  return `${n.toLocaleString()} ${n === 1 ? noun : plural}`;
}

export function pct(n: number): string {
  return `${Math.round(n * 100)}%`;
}

export function truncateMiddle(s: string, max = 48): string {
  if (s.length <= max) return s;
  const keep = Math.floor((max - 1) / 2);
  return `${s.slice(0, keep)}…${s.slice(-keep)}`;
}

/**
 * Bytes, in the terms of someone watching a download rather than in the terms
 * of the thing sending it. Until the host says how big the file is the total
 * is zero, and the honest answer then is that it has started — not a
 * percentage of a number nobody has.
 */
export function describeDownload(current: number, total: number): string {
  if (total <= 0) return "Starting the download…";
  const gb = (n: number) => `${(n / 1_000_000_000).toFixed(1)} GB`;
  return `${gb(current)} of ${gb(total)}`;
}
