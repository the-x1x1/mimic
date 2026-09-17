import { useState } from "react";
import * as Dialog from "@radix-ui/react-dialog";
import { Button, Field, InlineError } from "@mimic/ui";
import { FolderOpen, X } from "lucide-react";
import { ipc } from "@/lib/ipc";
import { useCreateSession } from "@/hooks/useSessions";
import { useStyles } from "@/hooks/useStyles";
import { useLightroomStatus } from "@/hooks/useLightroom";
import { useSystemStatus } from "@/hooks/useSystem";

type Source = "folder" | "lightroom";

export function NewSessionDialog({
  open,
  onOpenChange,
  onCreated,
}: {
  open: boolean;
  onOpenChange: (o: boolean) => void;
  onCreated: (sessionId: string) => void;
}) {
  const [name, setName] = useState("");
  const [source, setSource] = useState<Source>("folder");
  const [folder, setFolder] = useState<string | null>(null);
  const [scope, setScope] = useState<"selection" | "folder" | "collection" | "catalog">(
    "selection",
  );
  const [styleId, setStyleId] = useState<string>("");
  const [error, setError] = useState<string | null>(null);
  const create = useCreateSession();
  const styles = useStyles();
  const lr = useLightroomStatus();
  const system = useSystemStatus();
  const engineReady = system.data?.engine.state === "ready";
  const lrConnected = lr.data?.bridge.connected ?? false;
  const trained = (styles.data ?? []).filter((s) => s.activeVersion);

  const reset = () => {
    setName("");
    setFolder(null);
    setError(null);
    setSource("folder");
  };

  const submit = async () => {
    setError(null);
    if (!name.trim()) return setError("Name the session, e.g. “2026-09-12 Elopement”.");
    if (!engineReady)
      return setError("The analysis engine is not running yet. Check Settings › Diagnostics.");
    if (source === "folder" && !folder)
      return setError("Choose the folder with the new shoot's RAW files.");
    if (source === "lightroom" && !lrConnected)
      return setError(
        "Lightroom is not connected. Open Lightroom Classic with the Mimic plugin enabled.",
      );
    try {
      const session = await create.mutateAsync({
        name: name.trim(),
        source:
          source === "folder" ? { kind: "folder", path: folder! } : { kind: "lightroom", scope },
        styleId: styleId || null,
      });
      reset();
      onOpenChange(false);
      onCreated(session.id);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  return (
    <Dialog.Root open={open} onOpenChange={(o) => !create.isPending && onOpenChange(o)}>
      <Dialog.Portal>
        <Dialog.Overlay className="dialog-overlay" />
        <Dialog.Content className="dialog" aria-describedby={undefined}>
          <div className="dialog__head">
            <Dialog.Title>New Session</Dialog.Title>
            <Dialog.Close asChild>
              <Button variant="ghost" size="sm" icon={<X />} aria-label="Close" />
            </Dialog.Close>
          </div>
          <div className="stack gap-4">
            <Field
              label="Name"
              htmlFor="session-name"
              hint="A session is one shoot you want edited in your Style."
            >
              <input
                id="session-name"
                className="input"
                value={name}
                onChange={(e) => setName(e.target.value)}
                placeholder="2026-09-12 Elopement"
                autoFocus
              />
            </Field>
            <Field label="Photos from">
              <div className="segmented" role="radiogroup">
                <button
                  role="radio"
                  aria-checked={source === "folder"}
                  className={source === "folder" ? "seg seg--on" : "seg"}
                  onClick={() => setSource("folder")}
                >
                  Folder on disk
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
                hint="Read-only. To apply later, the same files must be imported in the Lightroom catalog; Mimic matches them by path."
              >
                <div className="path-row">
                  <code className="path">{folder ?? "No folder chosen"}</code>
                  <Button
                    icon={<FolderOpen />}
                    onClick={async () =>
                      setFolder((await ipc.pickFolder("Choose the shoot folder")) ?? folder)
                    }
                  >
                    Choose…
                  </Button>
                </div>
              </Field>
            ) : (
              <Field
                label="Photos"
                hint="Mimic captures the current develop state of each photo as its before-state."
              >
                <select
                  className="input"
                  value={scope}
                  onChange={(e) => setScope(e.target.value as typeof scope)}
                >
                  <option value="selection">Current selection</option>
                  <option value="collection">Current collection</option>
                  <option value="folder">Current folder</option>
                  <option value="catalog">Entire catalog</option>
                </select>
              </Field>
            )}
            <Field
              label="Style"
              hint={
                trained.length === 0
                  ? "No trained Style yet. You can still ingest and group; prediction needs a trained Style."
                  : "Which Style Brain predicts the edits. You can change it later."
              }
            >
              <select
                className="input"
                value={styleId}
                onChange={(e) => setStyleId(e.target.value)}
              >
                <option value="">Choose later</option>
                {trained.map((s) => (
                  <option key={s.id} value={s.id}>
                    {s.name} · v{s.activeVersion?.semanticVersion}
                  </option>
                ))}
              </select>
            </Field>
            {error ? <InlineError>{error}</InlineError> : null}
            <div className="row gap-2 end">
              <Dialog.Close asChild>
                <Button disabled={create.isPending}>Cancel</Button>
              </Dialog.Close>
              <Button variant="primary" onClick={submit} loading={create.isPending}>
                Create and ingest
              </Button>
            </div>
          </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
