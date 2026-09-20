import * as Dialog from "@radix-ui/react-dialog";
import { Button } from "@mimic/ui";
import { describeDeletion } from "@mimic/contracts";
import { useDeletePerson, useDeletionPreview } from "@/hooks/usePeople";

/**
 * Deletion is irreversible and takes more with it than people expect, so the
 * dialog states the consequences in sentences and cannot be confirmed until
 * the preview has loaded. The preview is computed by the same code path as the
 * deletion, so it cannot understate what will happen.
 */
export function DeletePersonDialog({
  participantId,
  name,
  onClose,
}: {
  participantId: string;
  name: string;
  onClose: () => void;
}) {
  const preview = useDeletionPreview(participantId);
  const remove = useDeletePerson();

  return (
    <Dialog.Root open onOpenChange={(o) => !o && onClose()}>
      <Dialog.Portal>
        <Dialog.Overlay className="dialog__overlay" />
        <Dialog.Content className="dialog">
          <Dialog.Title className="dialog__title">Delete {name}?</Dialog.Title>
          {preview.isLoading ? (
            <p>Working out what this would remove…</p>
          ) : preview.isError ? (
            <p className="danger">{(preview.error as Error).message}</p>
          ) : (
            <ul className="plain-list">
              {describeDeletion(preview.data!).map((line) => (
                <li key={line}>{line}</li>
              ))}
            </ul>
          )}
          <div className="row gap-2 dialog__actions">
            <Button
              variant="danger"
              disabled={!preview.data || remove.isPending}
              onClick={async () => {
                await remove.mutateAsync(participantId);
                onClose();
              }}
            >
              {remove.isPending ? "Deleting…" : "Delete permanently"}
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
