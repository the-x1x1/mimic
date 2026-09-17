import { useState } from "react";
import * as Dialog from "@radix-ui/react-dialog";
import { Button, Field, InlineError } from "@mimic/ui";
import { FolderOpen, X } from "lucide-react";
import { ipc } from "@/lib/ipc";
import { useCreateLibrary, useStartScan } from "@/hooks/useLibraries";
import { useCreateStyle } from "@/hooks/useStyles";
import { useLightroomStatus, useStartLightroomIngest } from "@/hooks/useLightroom";
import { useSystemStatus } from "@/hooks/useSystem";

type Source = "folder" | "lightroom";

export function NewStyleDialog({
  open,
  onOpenChange,
  onCreated,
}: {
  open: boolean;
  onOpenChange: (o: boolean) => void;
  onCreated: (styleId: string) => void;
}) {
  const [name, setName] = useState("");
  const [source, setSource] = useState<Source>("folder");
  const [folder, setFolder] = useState<string | null>(null);
  const [scope, setScope] = useState("selection");
  const [error, setError] = useState<string | null>(null);
  const createLibrary = useCreateLibrary();
  const createStyle = useCreateStyle();
  const startScan = useStartScan();
  const startIngest = useStartLightroomIngest();
  const lr = useLightroomStatus();
  const system = useSystemStatus();
  const busy =
    createLibrary.isPending ||
    createStyle.isPending ||
    startScan.isPending ||
    startIngest.isPending;
  const engineReady = system.data?.engine.state === "ready";
  const lrConnected = lr.data?.bridge.connected ?? false;

  const reset = () => {
    setName("");
    setFolder(null);
    setError(null);
    setSource("folder");
  };

  const submit = async () => {
    setError(null);
    if (!name.trim()) return setError("Give the Style a name, e.g. “Wedding Natural”.");
    if (!engineReady)
      return setError("The analysis engine is not running yet. Check Settings › Diagnostics.");
    try {
      if (source === "folder") {
        if (!folder)
          return setError(
            "Choose the folder that holds your edited RAW files and their .xmp sidecars.",
          );
        const lib = await createLibrary.mutateAsync({
          name: `${name.trim()} — folder`,
          sourceType: "folder_sidecars",
          rootPath: folder,
        });
        const style = await createStyle.mutateAsync({
          name: name.trim(),
          description: null,
          libraryId: lib.id,
        });
        await startScan.mutateAsync(lib.id);
        reset();
        onOpenChange(false);
        onCreated(style.id);
      } else {
        if (!lrConnected)
          return setError(
            "Lightroom is not connected. Open Lightroom Classic with the Mimic plugin enabled, then try again.",
          );
        const lib = await createLibrary.mutateAsync({
          name: `${name.trim()} — Lightroom`,
          sourceType: "lightroom_catalog",
          rootPath: null,
        });
        const style = await createStyle.mutateAsync({
          name: name.trim(),
          description: null,
          libraryId: lib.id,
        });
        await startIngest.mutateAsync({ libraryId: lib.id, scope });
        reset();
        onOpenChange(false);
        onCreated(style.id);
      }
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
            <Dialog.Title>New Style</Dialog.Title>
            <Dialog.Close asChild>
              <Button variant="ghost" size="sm" icon={<X />} aria-label="Close" />
            </Dialog.Close>
          </div>
          <div className="stack gap-4">
            <Field
              label="Name"
              htmlFor="style-name"
              hint="One editing intent per Style: “Wedding Natural”, “Automotive Clean”…"
            >
              <input
                id="style-name"
                className="input"
                value={name}
                onChange={(e) => setName(e.target.value)}
                placeholder="Wedding Natural"
                autoFocus
              />
            </Field>
            <Field label="Learn from">
              <div className="segmented" role="radiogroup">
                <button
                  role="radio"
                  aria-checked={source === "folder"}
                  className={source === "folder" ? "seg seg--on" : "seg"}
                  onClick={() => setSource("folder")}
                >
                  Folders + sidecars
                </button>
                <button
                  role="radio"
                  aria-checked={source === "lightroom"}
                  className={source === "lightroom" ? "seg seg--on" : "seg"}
                  onClick={() => setSource("lightroom")}
                >
                  Lightroom Classic {lrConnected ? "" : "(offline)"}
                </button>
              </div>
            </Field>
            {source === "folder" ? (
              <Field
                label="Folder"
                hint="Mimic reads RAW files and .xmp sidecars; nothing is written to the folder."
              >
                <div className="path-row">
                  <code className="path">{folder ?? "No folder chosen"}</code>
                  <Button
                    icon={<FolderOpen />}
                    onClick={async () =>
                      setFolder(
                        (await ipc.pickFolder("Choose a folder of edited photos")) ?? folder,
                      )
                    }
                  >
                    Choose…
                  </Button>
                </div>
              </Field>
            ) : (
              <Field
                label="Photos"
                hint="What to capture from the open catalog. Large catalogs take a while; start with a selection or collection."
              >
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
                Create and scan
              </Button>
            </div>
          </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
