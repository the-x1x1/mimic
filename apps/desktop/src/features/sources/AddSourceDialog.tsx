import { useState } from "react";
import * as Dialog from "@radix-ui/react-dialog";
import { Button, Field, InlineError } from "@mimic/ui";
import { CHANNEL_LABELS, Channel, type ValidationReport } from "@mimic/contracts";
import { ipc } from "@/lib/ipc";
import { useConnectors, useCreateSource, useStartImport } from "@/hooks/useSources";

/**
 * Adding a source is: pick a format, pick a file, see what Mimic found in it,
 * then decide. The validation step exists so nobody imports twelve years of
 * mail before discovering the export has no timestamps.
 */
export function AddSourceDialog({ onClose }: { onClose: () => void }) {
  const connectors = useConnectors();
  const create = useCreateSource();
  const startImport = useStartImport();

  const [connector, setConnector] = useState("");
  const [name, setName] = useState("");
  const [channel, setChannel] = useState<Channel>("email");
  const [location, setLocation] = useState<string | null>(null);
  const [report, setReport] = useState<ValidationReport | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [checking, setChecking] = useState(false);

  const chosen = connectors.data?.find((c) => c.connector === connector);

  async function pick() {
    if (!chosen) return;
    setError(null);
    const picked = await ipc.pickSourceFile(
      `Choose a ${chosen.displayName} file`,
      chosen.extensions,
    );
    if (!picked) return;
    setLocation(picked);
    if (!name) setName(picked.split(/[\\/]/).pop() ?? chosen.displayName);
    setChecking(true);
    try {
      setReport(await ipc.validateSourceFile(chosen.connector, picked));
    } catch (e) {
      setError((e as Error).message);
      setReport(null);
    } finally {
      setChecking(false);
    }
  }

  async function confirm() {
    const source = await create.mutateAsync({ connector, name, channel, location });
    await startImport.mutateAsync(source.id);
    onClose();
  }

  return (
    <Dialog.Root open onOpenChange={(o) => !o && onClose()}>
      <Dialog.Portal>
        <Dialog.Overlay className="dialog__overlay" />
        <Dialog.Content className="dialog dialog--wide">
          <Dialog.Title className="dialog__title">Add a source</Dialog.Title>

          <Field label="Format" htmlFor="connector">
            <select
              id="connector"
              value={connector}
              onChange={(e) => {
                setConnector(e.target.value);
                const c = connectors.data?.find((x) => x.connector === e.target.value);
                if (c) setChannel(c.channel as Channel);
                setReport(null);
                setLocation(null);
              }}
            >
              <option value="">Choose…</option>
              {(connectors.data ?? []).map((c) => (
                <option key={c.connector} value={c.connector}>
                  {c.displayName}
                </option>
              ))}
            </select>
          </Field>
          {chosen ? <p className="muted small">{chosen.description}</p> : null}

          {chosen ? (
            <>
              <div className="row gap-2">
                <Button onClick={pick}>
                  {location ? "Choose a different file" : "Choose a file"}
                </Button>
                {location ? <span className="mono small">{location}</span> : null}
              </div>

              {checking ? <p className="neutral">Reading the file…</p> : null}
              {error ? <InlineError>{error}</InlineError> : null}

              {report ? (
                <div className="stack gap-1">
                  <p>
                    {report.messages.toLocaleString()} messages across{" "}
                    {report.conversations.toLocaleString()} conversations
                    {report.earliest
                      ? `, ${report.earliest.slice(0, 10)} to ${report.latest?.slice(0, 10)}`
                      : ""}
                    .
                  </p>
                  {report.blockers.map((b) => (
                    <p key={b} className="danger">
                      {b}
                    </p>
                  ))}
                  {report.warnings.map((w) => (
                    <p key={w} className="muted small">
                      {w}
                    </p>
                  ))}
                  {report.frequentIdentifiers.length > 0 ? (
                    <p className="muted small">
                      Most frequent addresses:{" "}
                      {report.frequentIdentifiers
                        .slice(0, 5)
                        .map(([v, n]) => `${v} (${n})`)
                        .join(", ")}
                      . Make sure the ones that are yours are listed in Settings, or your own
                      messages will import as someone else&rsquo;s.
                    </p>
                  ) : null}
                </div>
              ) : null}

              <div className="row gap-2">
                <Field label="Name" htmlFor="source-name">
                  <input id="source-name" value={name} onChange={(e) => setName(e.target.value)} />
                </Field>
                <Field label="Channel" htmlFor="source-channel">
                  <select
                    id="source-channel"
                    value={channel}
                    onChange={(e) => setChannel(e.target.value as Channel)}
                  >
                    {Channel.options.map((c) => (
                      <option key={c} value={c}>
                        {CHANNEL_LABELS[c]}
                      </option>
                    ))}
                  </select>
                </Field>
              </div>
            </>
          ) : null}

          <div className="row gap-2 dialog__actions">
            <Button
              variant="primary"
              disabled={!report?.ok || !name.trim() || create.isPending}
              onClick={confirm}
            >
              Import
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
