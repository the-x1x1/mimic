import { useState } from "react";
import { Card } from "@mimic/ui";
import {
  editMapping,
  evaluationSet,
  familyErrors,
  formatNoTouch,
  primaryError,
  type ModelVersion,
  type NoTouchStats,
} from "@mimic/contracts";

function pick(versions: ModelVersion[], id: string): ModelVersion | undefined {
  return versions.find((v) => v.id === id);
}

/**
 * Two versions side by side: overall and per-family holdout error plus the
 * measured No-Touch Rate. Lower error is better; the delta column says which
 * one wins per family. Nothing is shown for a family neither version measured.
 */
export function VersionCompare({
  versions,
  noTouch,
}: {
  versions: ModelVersion[];
  noTouch: NoTouchStats[];
}) {
  const ready = versions.filter((v) => v.status === "ready" || v.status === "archived");
  const active = ready.find((v) => v.isActive) ?? ready[0];
  const other = ready.find((v) => v.id !== active?.id);
  const [aId, setA] = useState(active?.id ?? "");
  const [bId, setB] = useState(other?.id ?? "");
  const a = pick(ready, aId) ?? active;
  const b = pick(ready, bId) ?? other;
  if (!a || !b) {
    return <p className="muted small">Train a second version to compare.</p>;
  }
  const fa = familyErrors(a.metrics);
  const fb = familyErrors(b.metrics);
  const families = Object.keys({ ...fa, ...fb }).sort();
  const nt = (v: ModelVersion) => noTouch.find((n) => n.modelVersionId === v.id)?.rate ?? null;
  const label = (fam: string) => editMapping.families[fam]?.label ?? fam;
  const row = (
    name: string,
    va: number | null,
    vb: number | null,
    fmt: (n: number) => string,
    lowerIsBetter = true,
  ) => {
    const better =
      va === null || vb === null || va === vb
        ? "—"
        : (lowerIsBetter ? va < vb : va > vb)
          ? `v${a.semanticVersion}`
          : `v${b.semanticVersion}`;
    return (
      <tr key={name}>
        <td>{name}</td>
        <td className="num">{va === null ? "—" : fmt(va)}</td>
        <td className="num">{vb === null ? "—" : fmt(vb)}</td>
        <td className="num">{better}</td>
      </tr>
    );
  };
  return (
    <Card
      title="Compare versions"
      actions={
        <div className="row gap-2">
          <select
            className="input input--narrow"
            value={a.id}
            onChange={(e) => setA(e.target.value)}
            aria-label="Version A"
          >
            {ready.map((v) => (
              <option key={v.id} value={v.id}>
                v{v.semanticVersion}
              </option>
            ))}
          </select>
          <span className="muted">vs</span>
          <select
            className="input input--narrow"
            value={b.id}
            onChange={(e) => setB(e.target.value)}
            aria-label="Version B"
          >
            {ready.map((v) => (
              <option key={v.id} value={v.id}>
                v{v.semanticVersion}
              </option>
            ))}
          </select>
        </div>
      }
    >
      <table className="table">
        <thead>
          <tr>
            <th>Metric</th>
            <th className="num">v{a.semanticVersion}</th>
            <th className="num">v{b.semanticVersion}</th>
            <th className="num">Better</th>
          </tr>
        </thead>
        <tbody>
          {row(
            `Overall nMAE (${evaluationSet(a.metrics) ?? "n/a"})`,
            primaryError(a.metrics),
            primaryError(b.metrics),
            (n) => n.toFixed(4),
          )}
          {families.map((f) => row(label(f), fa[f] ?? null, fb[f] ?? null, (n) => n.toFixed(4)))}
          {row("No-Touch Rate (measured)", nt(a), nt(b), (n) => formatNoTouch(n), false)}
        </tbody>
      </table>
      <p className="muted small mt-2">
        Error is holdout nMAE (fraction of each control's range, lower is better). No-Touch Rate is
        measured from synced sessions, not predicted.
      </p>
    </Card>
  );
}
