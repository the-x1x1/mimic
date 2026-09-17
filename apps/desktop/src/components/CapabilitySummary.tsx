import { Badge } from "@mimic/ui";
import type { CapabilityMatrix } from "@mimic/contracts";

export function CapabilitySummary({ matrix, live }: { matrix: CapabilityMatrix; live: boolean }) {
  const families = Object.entries(matrix.familySummary).sort(([a], [b]) => a.localeCompare(b));
  return (
    <div className="cap">
      <div className="row gap-2 wrap">
        <Badge tone={live ? "success" : "neutral"}>{live ? "live" : "last known"}</Badge>
        <Badge>Lightroom {matrix.lightroomVersion}</Badge>
        <Badge>plugin {matrix.pluginVersion}</Badge>
        <Badge tone={matrix.canRead ? "success" : "danger"}>
          read {matrix.canRead ? "yes" : "no"}
        </Badge>
        <Badge tone={matrix.canApply ? "success" : "warning"}>
          apply {matrix.canApply ? "yes" : "no"}
        </Badge>
        <Badge tone={matrix.canSnapshot ? "success" : "warning"}>
          snapshots {matrix.canSnapshot ? "yes" : "no"}
        </Badge>
        <Badge tone="neutral" title={matrix.schemaVersion}>
          schema {matrix.schemaVersion.slice(0, 12)}
        </Badge>
      </div>
      {!matrix.probeHadPhoto ? (
        <p className="muted small">
          No photo was selected during the capability probe. Select a photo in Lightroom and press
          Test connection to see per-control support.
        </p>
      ) : null}
      <table className="table">
        <thead>
          <tr>
            <th>Family</th>
            <th>Writable</th>
            <th>Observed only</th>
            <th>Unsupported</th>
          </tr>
        </thead>
        <tbody>
          {families.map(([key, f]) => (
            <tr key={key}>
              <td>{f.label}</td>
              <td className={f.supported > 0 ? "good" : "muted"}>{f.supported}</td>
              <td className="muted">{f.observedNotWritable}</td>
              <td className={f.unsupported > 0 ? "warn" : "muted"}>{f.unsupported}</td>
            </tr>
          ))}
        </tbody>
      </table>
      <p className="muted small">Masks and local adjustments: {matrix.masks}</p>
    </div>
  );
}
