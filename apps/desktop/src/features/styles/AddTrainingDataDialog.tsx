import { useState } from "react";
import * as Dialog from "@radix-ui/react-dialog";
import { Button, Field, InlineError } from "@mimic/ui";
import { FolderOpen, X } from "lucide-react";
import { ipc } from "@/lib/ipc";
import { useCreateLibrary, useStartScan } from "@/hooks/useLibraries";
import { useAttachLibrary } from "@/hooks/useStyles";
import { useLightroomStatus, useStartLightroomIngest } from "@/hooks/useLightroom";

export function AddTrainingDataDialog({
  styleId,
  open,
  onOpenChange,
}: {
  styleId: string;
  open: boolean;
  onOpenChange: (o: boolean) => void;
}) {
  const [source, setSource] = useState<"folder" | "lightroom">("folder");
  const [folder, setFolder] = useState<string | null>(null);
  const [scope, setScope] = useState("selection");
  const [error, setError] = useState<string | null>(null);
  const createLibrary = useCreateLibrary();
  const attach = useAttachLibrary();
  const scan = useStartScan();
  const ingest = useStartLightroomIngest();
  const lr = useLightroomStatus();
  const busy = createLibrary.isPending || attach.isPending || scan.isPending || ingest.isPending;

  const submit = async () => {
    setError(null);
    try {
      if (source === "folder") {
        if (!folder) return setError("Choose a folder first.");
        const lib = await createLibrary.mutateAsync({
          name: folder.split(/[\\/]/).filter(Boolean).pop() ?? "Folder",
          sourceType: "folder_sidecars",
          rootPath: folder,
        });
        await attach.mutateAsync({ styleId, libraryId: lib.id });
        await scan.mutateAsync(lib.id);
      } else {
        if (!lr.data?.bridge.connected) return setError("Lightroom is not connected.");
        const lib = await createLibrary.mutateAsync({
          name: `Lightroom — ${lr.data.bridge.connection?.catalogName ?? "catalog"}`,
          sourceType: "lightroom_catalog",
          rootPath: null,
        });
        await attach.mutateAsync({ styleId, libraryId: lib.id });
        await ingest.mutateAsync({ libraryId: lib.id, scope });
      }
      setFolder(null);
      onOpenChange(false);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  return (
    <Dialog.Root
      open={open}
      onOpenChange={(o) => {
        if (!busy) onOpenChange(o);
      }}
    >
      <Dialog.Portal>
        <Dialog.Overlay className="dialog-overlay" />
        <Dialog.Content className="dialog" aria-describedby={undefined}>
          <div className="dialog__head">
            <Dialog.Title>Add Training Data</Dialog.Title>
            <Dialog.Close asChild>
              <Button variant="ghost" size="sm" icon={<X />} aria-label="Close" />
            </Dialog.Close>
          </div>
          <div className="stack gap-4">
            <div className="segmented" role="radiogroup">
              <button
                role="radio"
                aria-checked={source === "folder"}
                className={source === "folder" ? "seg seg--on" : "seg"}
                onClick={() => setSource("folder")}
              >
                Folder + sidecars
              </button>
              <button
                role="radio"
                aria-checked={source === "lightroom"}
                className={source === "lightroom" ? "seg seg--on" : "seg"}
                onClick={() => setSource("lightroom")}
              >
                Lightroom Classic
              </button>
            </div>
            {source === "folder" ? (
              <Field label="Folder">
                <div className="path-row">
                  <code className="path">{folder ?? "No folder chosen"}</code>
                  <Button
                    icon={<FolderOpen />}
                    onClick={async () => setFolder((await ipc.pickFolder()) ?? folder)}
                  >
                    Choose…
                  </Button>
                </div>
              </Field>
            ) : (
              <Field label="Photos">
                <select className="input" value={scope} onChange={(e) => setScope(e.target.value)}>
                  <option value="selection">Current selection</option>
                  <option value="collection">Current collection</option>
                  <option value="folder">Current folder</option>
                  <option value="catalog">Entire catalog</option>
                </select>
              </Field>
            )}
            {error ? <InlineError>{error}</InlineError> : null}
            <div className="row gap-2 end">
              <Dialog.Close asChild>
                <Button disabled={busy}>Cancel</Button>
              </Dialog.Close>
              <Button variant="primary" onClick={submit} loading={busy}>
                Add and scan
              </Button>
            </div>
          </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
