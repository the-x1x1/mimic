import { Badge } from "@mimic/ui";
import type { AssetRow } from "@mimic/contracts";
import { previewUrl } from "@/lib/ipc";
import { ImageOff } from "lucide-react";

export function PhotoGrid({
  assets,
  onSelect,
}: {
  assets: AssetRow[];
  onSelect?: (a: AssetRow) => void;
}) {
  return (
    <div className="photo-grid" role="list">
      {assets.map((a) => (
        <PhotoTile key={a.id} asset={a} onSelect={onSelect} />
      ))}
    </div>
  );
}

export function PhotoTile({
  asset,
  onSelect,
}: {
  asset: AssetRow;
  onSelect?: (a: AssetRow) => void;
}) {
  const src = previewUrl(asset.previewPath);
  return (
    <button
      className="tile"
      role="listitem"
      onClick={() => onSelect?.(asset)}
      title={asset.sourcePath}
    >
      <div className="tile__image">
        {src ? (
          <img src={src} alt="" loading="lazy" decoding="async" />
        ) : (
          <div className="tile__placeholder">
            <ImageOff size={18} />
          </div>
        )}
      </div>
      <div className="tile__meta">
        <span className="tile__name">{asset.fileName}</span>
        {asset.hasEdits ? (
          <Badge tone="success" title={`edits from ${asset.editSource}`}>
            edited
          </Badge>
        ) : (
          <Badge tone="neutral">no edits</Badge>
        )}
      </div>
    </button>
  );
}
