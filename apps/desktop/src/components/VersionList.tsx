import { Badge, Button } from "@mimic/ui";
import type { ModelVersion } from "@mimic/contracts";
import { controlMae, evaluationSet, primaryError } from "@mimic/contracts";
import { formatDate } from "@/lib/format";

/** Immutable model versions with real holdout metrics; activate = rollback path. */
export function VersionList({
  versions,
  onActivate,
  onArchive,
  busy,
}: {
  versions: ModelVersion[];
  onActivate: (id: string) => void;
  onArchive: (id: string) => void;
  busy?: boolean;
}) {
  return (
    <table className="table">
      <thead>
        <tr>
          <th>Version</th>
          <th>Status</th>
          <th>Evaluated on</th>
          <th>Overall error (nMAE)</th>
          <th>Exposure MAE</th>
          <th>Beats median</th>
          <th>Trained</th>
          <th />
        </tr>
      </thead>
      <tbody>
        {versions.map((v) => {
          const err = primaryError(v.metrics);
          const set = evaluationSet(v.metrics);
          const exp = controlMae(v.metrics, "hybrid", "tone.exposure");
          const manifest = v.artifactManifest as {
            beatsBaselines?: { beatsGlobalMedian?: boolean; evaluated?: boolean };
          } | null;
          const beats = manifest?.beatsBaselines?.evaluated
            ? manifest.beatsBaselines.beatsGlobalMedian
              ? "yes"
              : "no"
            : "—";
          const failed = v.status === "failed";
          const errorMsg = failed
            ? ((v.metrics as { error?: { message?: string } })?.error?.message ?? "training failed")
            : null;
          return (
            <tr key={v.id}>
              <td>
                <strong>v{v.semanticVersion}</strong>
                {v.isActive ? (
                  <Badge tone="success" className="ml-2">
                    active
                  </Badge>
                ) : null}
              </td>
              <td>
                <Badge
                  tone={
                    v.status === "ready"
                      ? "neutral"
                      : v.status === "training"
                        ? "info"
                        : v.status === "failed"
                          ? "danger"
                          : "neutral"
                  }
                >
                  {v.status}
                </Badge>
                {errorMsg ? <div className="muted small">{errorMsg}</div> : null}
              </td>
              <td className="muted">{set ?? "—"}</td>
              <td className="mono">{err !== null ? err.toFixed(4) : "—"}</td>
              <td className="mono">{exp !== null ? `${exp.toFixed(2)} EV` : "—"}</td>
              <td className={beats === "yes" ? "good" : beats === "no" ? "warn" : "muted"}>
                {beats}
              </td>
              <td className="muted">{formatDate(v.createdAt)}</td>
              <td className="row gap-2 end">
                {v.status === "ready" && !v.isActive ? (
                  <Button size="sm" onClick={() => onActivate(v.id)} disabled={busy}>
                    Activate
                  </Button>
                ) : null}
                {v.status === "ready" && !v.isActive ? (
                  <Button size="sm" variant="ghost" onClick={() => onArchive(v.id)} disabled={busy}>
                    Archive
                  </Button>
                ) : null}
              </td>
            </tr>
          );
        })}
      </tbody>
    </table>
  );
}
