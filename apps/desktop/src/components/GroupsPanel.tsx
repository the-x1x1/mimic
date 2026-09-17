import { useState } from "react";
import { Badge, Button, Card, InlineError } from "@mimic/ui";
import { Check, Merge, Pencil, Star, X } from "lucide-react";
import type { GroupEdit, GroupStats, SceneCluster, SessionPhoto } from "@mimic/contracts";

/**
 * Scene groups with per-group confidence and the four edits a photographer
 * can make: rename, choose a reference photo, merge into another group, move
 * the selected photos. Every edit marks the group edited so the page can say
 * the last prediction predates it.
 */
export function GroupsPanel({
  clusters,
  stats,
  photos,
  selected,
  onEdit,
  busy,
  onFilter,
  activeFilter,
}: {
  clusters: SceneCluster[];
  stats: GroupStats[];
  photos: SessionPhoto[];
  /** Currently selected photo ids in the grid (for Move / Reference). */
  selected: string[];
  onEdit: (edit: GroupEdit) => void;
  busy?: boolean;
  onFilter: (clusterId: string) => void;
  activeFilter: string;
}) {
  const [renaming, setRenaming] = useState<string | null>(null);
  const [label, setLabel] = useState("");
  const [mergeFrom, setMergeFrom] = useState<string | null>(null);
  const [newLabel, setNewLabel] = useState("");
  const byId = new Map(stats.map((s) => [s.clusterId, s]));
  const fileName = (assetId: string | null) =>
    photos.find((p) => p.asset.id === assetId)?.asset.fileName ?? null;
  const selectedOne = selected.length === 1 ? selected[0]! : null;
  const selectedCluster = selectedOne
    ? (photos.find((p) => p.asset.id === selectedOne)?.clusterId ?? null)
    : null;

  if (clusters.length === 0) {
    return <p className="muted small">Run Analyze scenes to split the session into groups.</p>;
  }
  return (
    <Card title={`Scene groups (${clusters.length})`}>
      <table className="table groups-table">
        <thead>
          <tr>
            <th>Group</th>
            <th className="num">Photos</th>
            <th className="num">Confidence</th>
            <th className="num">Attention</th>
            <th>Reference</th>
            <th></th>
          </tr>
        </thead>
        <tbody>
          {clusters.map((c) => {
            const s = byId.get(c.id);
            const attention = s ? s.lowConfidence + s.outOfDistribution + s.outliers : 0;
            const refName = fileName(c.referenceAssetId);
            return (
              <tr
                key={c.id}
                className={activeFilter === c.id ? "groups-table__row--on" : undefined}
              >
                <td>
                  {renaming === c.id ? (
                    <span className="row gap-2">
                      <input
                        className="input input--narrow"
                        value={label}
                        onChange={(e) => setLabel(e.target.value)}
                        aria-label="Group name"
                        autoFocus
                      />
                      <Button
                        size="sm"
                        icon={<Check />}
                        aria-label="Save name"
                        onClick={() => {
                          onEdit({ kind: "rename", clusterId: c.id, label });
                          setRenaming(null);
                        }}
                        disabled={busy || !label.trim()}
                      />
                      <Button
                        size="sm"
                        variant="ghost"
                        icon={<X />}
                        aria-label="Cancel"
                        onClick={() => setRenaming(null)}
                      />
                    </span>
                  ) : (
                    <button
                      className="link-button"
                      onClick={() => onFilter(c.id)}
                      title="Show only this group"
                    >
                      {c.label}
                    </button>
                  )}
                  {c.editedAt ? (
                    <Badge tone="neutral" className="ml-2" title={`edited ${c.editedAt}`}>
                      edited
                    </Badge>
                  ) : null}
                </td>
                <td className="num">{c.assetCount}</td>
                <td className="num">
                  {s?.meanConfidence !== null && s?.meanConfidence !== undefined
                    ? `${Math.round(s.meanConfidence * 100)}%`
                    : "—"}
                  {s?.minConfidence !== null && s?.minConfidence !== undefined ? (
                    <span className="muted small"> (min {Math.round(s.minConfidence * 100)}%)</span>
                  ) : null}
                </td>
                <td className="num">
                  {attention > 0 ? (
                    <Badge
                      tone="warning"
                      title={`${s?.lowConfidence} low confidence · ${s?.outOfDistribution} unfamiliar · ${s?.outliers} group outliers`}
                    >
                      {attention}
                    </Badge>
                  ) : s && s.predicted > 0 ? (
                    <Badge tone="success">0</Badge>
                  ) : (
                    "—"
                  )}
                </td>
                <td>
                  {refName ? (
                    <span className="row gap-2">
                      <Star size={12} /> <span className="mono small">{refName}</span>
                      <Button
                        size="sm"
                        variant="ghost"
                        icon={<X />}
                        aria-label="Clear reference"
                        onClick={() =>
                          onEdit({ kind: "setReference", clusterId: c.id, assetId: null })
                        }
                        disabled={busy}
                      />
                    </span>
                  ) : (
                    <Button
                      size="sm"
                      icon={<Star />}
                      disabled={busy || !selectedOne || selectedCluster !== c.id}
                      title={
                        selectedOne && selectedCluster === c.id
                          ? "Use the selected photo as this group's reference"
                          : "Select one photo of this group first"
                      }
                      onClick={() =>
                        onEdit({ kind: "setReference", clusterId: c.id, assetId: selectedOne })
                      }
                    >
                      Use selected
                    </Button>
                  )}
                </td>
                <td className="num">
                  <span className="row gap-2 end">
                    <Button
                      size="sm"
                      variant="ghost"
                      icon={<Pencil />}
                      aria-label={`Rename ${c.label}`}
                      onClick={() => {
                        setRenaming(c.id);
                        setLabel(c.label);
                      }}
                      disabled={busy}
                    />
                    {mergeFrom === null ? (
                      <Button
                        size="sm"
                        variant="ghost"
                        icon={<Merge />}
                        aria-label={`Merge ${c.label} into another group`}
                        title="Merge this group into…"
                        onClick={() => setMergeFrom(c.id)}
                        disabled={busy || clusters.length < 2}
                      />
                    ) : mergeFrom === c.id ? (
                      <Button size="sm" variant="ghost" onClick={() => setMergeFrom(null)}>
                        cancel
                      </Button>
                    ) : (
                      <Button
                        size="sm"
                        variant="primary"
                        onClick={() => {
                          onEdit({ kind: "merge", into: c.id, from: mergeFrom });
                          setMergeFrom(null);
                        }}
                        disabled={busy}
                      >
                        merge here
                      </Button>
                    )}
                    {selected.length > 0 && selectedCluster !== c.id ? (
                      <Button
                        size="sm"
                        onClick={() =>
                          onEdit({ kind: "move", assetIds: selected, into: c.id, label: null })
                        }
                        disabled={busy}
                        title={`Move ${selected.length} selected photo(s) into ${c.label}`}
                      >
                        Move {selected.length} here
                      </Button>
                    ) : null}
                  </span>
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
      {mergeFrom ? (
        <InlineError title="Merging">
          Choose the group that should absorb “{clusters.find((c) => c.id === mergeFrom)?.label}”.
          Its name and reference are kept; the merged group disappears.
        </InlineError>
      ) : null}
      {selected.length > 0 ? (
        <div className="row gap-2 mt-3">
          <input
            className="input input--narrow"
            placeholder="New group name"
            value={newLabel}
            onChange={(e) => setNewLabel(e.target.value)}
            aria-label="New group name"
          />
          <Button
            size="sm"
            onClick={() => {
              onEdit({ kind: "move", assetIds: selected, into: null, label: newLabel || null });
              setNewLabel("");
            }}
            disabled={busy}
          >
            Split {selected.length} selected into a new group
          </Button>
        </div>
      ) : (
        <p className="muted small mt-2">
          Select photos in the grid to move them between groups or split them off; select one photo
          to make it a group's reference. Reference photos steer the group's colour and white
          balance and are never blended themselves.
        </p>
      )}
    </Card>
  );
}
