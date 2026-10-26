import type { ImapProbe } from "@mimic/contracts";

/** What a mailbox's folders hold, as a look at them found (0.10.0-alpha.51). */
export function describeProbe(probe: ImapProbe): string {
  const counts = probe.counts.map(([name, n]) => `${name} (${n.toLocaleString()})`);
  const reads = counts.length > 0 ? ` I'd read ${counts.join(", ")}.` : "";
  const first = probe.warnings[0];
  return `Signed in.${reads}${first ? ` ${first}` : ""}`;
}
