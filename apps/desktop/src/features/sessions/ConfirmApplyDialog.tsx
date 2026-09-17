import * as Dialog from "@radix-ui/react-dialog";
import { Badge, Button, InlineError } from "@mimic/ui";
import { ShieldCheck, X } from "lucide-react";
import type { ApplyPreflight } from "@mimic/contracts";

/**
 * The only place Mimic asks before touching a catalog. It states exactly what
 * will happen and refuses (with the backend's reasons) when any safety check
 * fails. Nothing here is decorative: every line maps to a check in
 * mimic-core::sessions::apply_preflight or to the plugin's apply path.
 */
export function ConfirmApplyDialog({
  open,
  onOpenChange,
  preflight,
  photoCount,
  onConfirm,
  busy,
  scopeLabel,
}: {
  open: boolean;
  onOpenChange: (o: boolean) => void;
  preflight: ApplyPreflight | undefined;
  photoCount: number;
  onConfirm: () => void;
  busy: boolean;
  scopeLabel: string;
}) {
  const pf = preflight;
  const batches = pf ? Math.ceil(pf.candidateCount / Math.max(1, pf.batchSize)) : 0;
  return (
    <Dialog.Root open={open} onOpenChange={(o) => !busy && onOpenChange(o)}>
      <Dialog.Portal>
        <Dialog.Overlay className="dialog-overlay" />
        <Dialog.Content className="dialog" aria-describedby={undefined}>
          <div className="dialog__head">
            <Dialog.Title>Apply to Lightroom</Dialog.Title>
            <Dialog.Close asChild>
              <Button variant="ghost" size="sm" icon={<X />} aria-label="Close" />
            </Dialog.Close>
          </div>
          <div className="stack gap-3">
            <p>
              Mimic will write predicted develop settings for <strong>{photoCount}</strong> photo
              {photoCount === 1 ? "" : "s"} ({scopeLabel}) into the open Lightroom catalog.
            </p>
            <ul className="plain-list safety-list">
              <li>
                <ShieldCheck size={14} /> A <em>Mimic Before</em> develop snapshot is created on
                every photo first. If the snapshot fails, that photo is not touched.
              </li>
              <li>
                <ShieldCheck size={14} /> Settings are applied as a plugin preset through
                Lightroom's SDK — never by editing the catalog file or XMP.
              </li>
              <li>
                <ShieldCheck size={14} /> Every photo is read back and compared to what was sent.
                Only a match counts as applied; mismatches are flagged for review.
              </li>
              <li>
                <ShieldCheck size={14} /> Photos go in batches of {pf?.batchSize ?? 25}
                {batches > 1 ? ` (${batches} batches)` : ""}; you can stop between batches. The
                recorded before-values allow a one-click Restore afterwards.
              </li>
            </ul>
            {pf ? (
              <div className="row gap-2 wrap">
                <Badge tone={pf.lightroomConnected ? "success" : "danger"}>
                  {pf.lightroomConnected ? "Lightroom connected" : "Lightroom offline"}
                </Badge>
                <Badge tone="info">{pf.writableControls} writable controls</Badge>
                {pf.staleCount > 0 ? <Badge tone="danger">{pf.staleCount} stale</Badge> : null}
              </div>
            ) : null}
            {pf && !pf.ok ? (
              <InlineError title="Apply is not possible right now">
                <ul className="plain-list">
                  {pf.blockers.map((b) => (
                    <li key={b}>{b}</li>
                  ))}
                </ul>
              </InlineError>
            ) : null}
            {pf && pf.warnings.length > 0 ? (
              <div className="dq__warnings">
                {pf.warnings.map((w) => (
                  <span key={w}>{w}</span>
                ))}
              </div>
            ) : null}
            <div className="row gap-2 end">
              <Dialog.Close asChild>
                <Button disabled={busy}>Cancel</Button>
              </Dialog.Close>
              <Button
                variant="primary"
                onClick={onConfirm}
                loading={busy}
                disabled={!pf || !pf.ok || photoCount === 0}
              >
                Apply {photoCount} photo{photoCount === 1 ? "" : "s"}
              </Button>
            </div>
          </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
