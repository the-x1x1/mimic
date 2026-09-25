import { useId, useState } from "react";
import * as Dialog from "@radix-ui/react-dialog";
import { Button, Field, InlineError } from "@mimic/ui";
import { CHANNEL_LABELS, Channel, yoursAs, type ValidationReport } from "@mimic/contracts";
import { ipc } from "@/lib/ipc";
import { useIdentity, useSetIdentity } from "@/hooks/usePeople";
import { useConnectors, useCreateSource, useStartImport } from "@/hooks/useSources";
import { WhoIsYou } from "./WhoIsYou";
import { AddAddressForm } from "@/features/identity/AddAddressForm";

/**
 * Adding a source is: pick a format, pick a file, see what Mimic found in it,
 * then decide. The validation step exists so nobody imports twelve years of
 * mail before discovering the export has no timestamps. A source that is one
 * writer's (a Discord package) waits until that writer is the user, since
 * read as someone else's, all of it would wait for a reply.
 */
export function AddSourceDialog({ onClose }: { onClose: () => void }) {
  const connectors = useConnectors();
  const create = useCreateSource();
  const startImport = useStartImport();
  const identity = useIdentity();
  const saveIdentity = useSetIdentity();
  const [displayName, setDisplayName] = useState("");
  const whyNot = useId();

  const [connector, setConnector] = useState("");
  const [name, setName] = useState("");
  const [channel, setChannel] = useState<Channel>("email");
  const [location, setLocation] = useState<{ path: string; kind: "file" | "folder" } | null>(null);
  const [report, setReport] = useState<ValidationReport | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [checking, setChecking] = useState(false);

  const chosen = connectors.data?.find((c) => c.connector === connector);
  const kinds: ("file" | "folder")[] = !chosen
    ? []
    : chosen.locationKind === "fileOrFolder"
      ? ["file", "folder"]
      : [chosen.locationKind];
  const identifiers = identity.data?.identifiers ?? [];
  const notYetYours =
    report?.oneWriter === true && !report.names.some((w) => yoursAs(w, identifiers) !== null);

  async function pick(kind: "file" | "folder") {
    if (!chosen) return;
    setError(null);
    const picked =
      kind === "folder"
        ? await ipc.pickSourceFolder(`Choose the unzipped ${chosen.displayName} folder`)
        : await ipc.pickSourceFile(`Choose a ${chosen.displayName} file`, chosen.extensions);
    if (!picked) return;
    setLocation({ path: picked, kind });
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
    const source = await create.mutateAsync({
      connector,
      name,
      channel,
      location: location?.path ?? null,
    });
    await startImport.mutateAsync(source.id);
    onClose();
  }

  return (
    <Dialog.Root open onOpenChange={(o) => !o && onClose()}>
      <Dialog.Portal>
        <Dialog.Overlay className="dialog__overlay" />
        <Dialog.Content className="dialog dialog--wide">
          <Dialog.Title className="dialog__title">Add a source</Dialog.Title>
          <Dialog.Description className="muted small">
            Choose an export you already have. Preview its messages, identify your writing, then
            import. No mailbox connection or model download is needed.
          </Dialog.Description>

          {identity.isSuccess && !identity.data ? (
            <>
              <Field label="Your name" htmlFor="import-your-name">
                <input
                  id="import-your-name"
                  value={displayName}
                  onChange={(e) => setDisplayName(e.target.value)}
                />
              </Field>
              <Button
                disabled={!displayName.trim() || saveIdentity.isPending}
                onClick={() => saveIdentity.mutate(displayName.trim())}
              >
                Save name
              </Button>
              <p className="muted small">
                Save a name before identifying your messages. No email address is required for chat
                exports.
              </p>
            </>
          ) : null}
          {identity.isError ? <InlineError>{identity.error.message}</InlineError> : null}
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
                {kinds.map((kind) => (
                  <Button key={kind} onClick={() => void pick(kind)}>
                    {location?.kind === kind
                      ? `Choose a different ${kind}`
                      : kind === "folder"
                        ? kinds.length > 1
                          ? "Choose the unzipped folder"
                          : "Choose a folder"
                        : "Choose a file"}
                  </Button>
                ))}
                {location ? <span className="mono small">{location.path}</span> : null}
              </div>

              {checking ? <p className="neutral">Reading it…</p> : null}
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
                  {report.sentFolder > 0 ? (
                    <p className="muted small">
                      {report.sentFolder === 1
                        ? "1 of them is from your Sent folder."
                        : `${report.sentFolder.toLocaleString()} of them are from your Sent folder.`}{" "}
                      If any are from an address you haven&rsquo;t given me, I&rsquo;ll ask whether
                      it&rsquo;s you.
                    </p>
                  ) : null}
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
                  {identity.data && report.names.length > 0 ? (
                    <WhoIsYou names={report.names} connector={connector} />
                  ) : null}
                  {report.names.length === 0 && report.frequentIdentifiers.length > 0 ? (
                    <>
                      <p className="muted small">
                        Most frequent addresses:{" "}
                        {report.frequentIdentifiers
                          .slice(0, 5)
                          .map(([v, n]) => `${v} (${n})`)
                          .join(", ")}
                        . Add the address you wrote from below so Mimic can recognize your messages.
                      </p>
                      {identity.data ? <AddAddressForm inputLabel="Your sending address" /> : null}
                      {identifiers.length > 0 ? (
                        <p className="muted small">
                          Your identifiers: {identifiers.map((i) => i.value).join(", ")}
                        </p>
                      ) : null}
                    </>
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

          {report?.ok && notYetYours ? (
            <p id={whyNot} className="muted small">
              Say the account is yours before importing it.
            </p>
          ) : null}
          <div className="row gap-2 dialog__actions">
            <Button
              variant="primary"
              disabled={
                !identity.data ||
                !report?.ok ||
                notYetYours ||
                !name.trim() ||
                create.isPending ||
                startImport.isPending
              }
              aria-describedby={report?.ok && notYetYours ? whyNot : undefined}
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
