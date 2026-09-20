import { useState } from "react";
import * as Dialog from "@radix-ui/react-dialog";
import { useQueryClient } from "@tanstack/react-query";
import { Button } from "@mimic/ui";
import { ipc } from "@/lib/ipc";
import { qk } from "@/app/queryClient";
import { toast } from "@/state/toast";

const PHRASE = "delete everything";

/** Typing the phrase is the point: this cannot be undone and takes everything. */
export function DeleteEverythingDialog({ onClose }: { onClose: () => void }) {
  const [typed, setTyped] = useState("");
  const [working, setWorking] = useState(false);
  const qc = useQueryClient();

  return (
    <Dialog.Root open onOpenChange={(o) => !o && onClose()}>
      <Dialog.Portal>
        <Dialog.Overlay className="dialog__overlay" />
        <Dialog.Content className="dialog">
          <Dialog.Title className="dialog__title">
            Delete everything Mimic has imported?
          </Dialog.Title>
          <p>
            Every message, conversation, person, voice profile and draft will be removed from this
            computer. Your settings, your own identity and your model configuration are kept.
          </p>
          <p>The original files you imported from are not touched.</p>
          <p className="neutral">
            Type <strong>{PHRASE}</strong> to confirm.
          </p>
          <input value={typed} onChange={(e) => setTyped(e.target.value)} autoFocus />
          <div className="row gap-2 dialog__actions">
            <Button
              variant="danger"
              disabled={typed.trim().toLowerCase() !== PHRASE || working}
              onClick={async () => {
                setWorking(true);
                try {
                  const report = await ipc.deleteAllCommunicationData();
                  for (const key of [
                    qk.sources,
                    qk.people,
                    qk.voice,
                    qk.system,
                    qk.onboarding,
                    qk.drafts,
                  ]) {
                    qc.invalidateQueries({ queryKey: key });
                  }
                  toast.info(
                    "Deleted",
                    `${report.messages.toLocaleString()} messages and ${report.participants} people removed.`,
                  );
                  onClose();
                } finally {
                  setWorking(false);
                }
              }}
            >
              {working ? "Deleting…" : "Delete everything"}
            </Button>
            <Button variant="ghost" onClick={onClose}>
              Cancel
            </Button>
          </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
