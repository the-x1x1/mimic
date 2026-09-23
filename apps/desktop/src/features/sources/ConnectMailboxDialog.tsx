import { useState } from "react";
import * as Dialog from "@radix-ui/react-dialog";
import { useQueryClient } from "@tanstack/react-query";
import { Button, Field, InlineError } from "@mimic/ui";
import { guessMailHost, type ImapAccount, type ImapProbe } from "@mimic/contracts";
import { ipc } from "@/lib/ipc";
import { qk } from "@/app/queryClient";
import { toast } from "@/state/toast";
import { useProviderState } from "@/hooks/useCompose";

/**
 * Connecting a mailbox is: say which one, log in once to look around, see
 * what will be read, then decide. The look-around step exists so a missing
 * sent folder — which would leave Mimic nothing of yours to learn from — is
 * known before anything is imported, not discovered a week later.
 *
 * The password is an app password, and the copy says so first, because the
 * usual one fails on every major provider and the error from the server does
 * not say why.
 */
export function ConnectMailboxDialog({ onClose }: { onClose: () => void }) {
  const qc = useQueryClient();
  const [username, setUsername] = useState("");
  const [host, setHost] = useState("");
  const [port, setPort] = useState(993);
  const [password, setPassword] = useState("");
  const [probe, setProbe] = useState<ImapProbe | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState<"checking" | "connecting" | null>(null);
  const [isMine, setIsMine] = useState(true);
  // Said only when the store reports it: until then, and on a build that
  // does not seal, the note claims nothing it cannot back.
  const sealed = useProviderState().data?.credentials.protection === "account";

  const known = guessMailHost(username);
  const account: ImapAccount = {
    host: host.trim() || known?.host || "",
    port: host.trim() ? port : (known?.port ?? port),
    username: username.trim(),
    security: "tls",
  };
  const unsupported = known?.supported === false;
  const ready = !unsupported && account.host !== "" && account.username !== "" && password !== "";

  async function look() {
    setError(null);
    setProbe(null);
    setBusy("checking");
    try {
      setProbe(await ipc.probeMailbox(account, password));
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setBusy(null);
    }
  }

  async function connect() {
    setError(null);
    setBusy("connecting");
    try {
      await ipc.connectMailbox(account, password, isMine);
      for (const key of [
        qk.sources,
        qk.jobs,
        qk.onboarding,
        qk.dashboard,
        qk.settings,
        qk.providers,
      ]) {
        void qc.invalidateQueries({ queryKey: key });
      }
      toast.success("Connected.", "I'm reading your mail now. Nothing in the mailbox changes.");
      onClose();
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setBusy(null);
    }
  }

  return (
    <Dialog.Root open onOpenChange={(o) => !o && onClose()}>
      <Dialog.Portal>
        <Dialog.Overlay className="dialog__overlay" />
        <Dialog.Content className="dialog dialog--wide">
          <Dialog.Title className="dialog__title">Connect a mailbox</Dialog.Title>
          <Dialog.Description className="neutral">
            I&rsquo;ll read your inbox and your sent mail, and check for new mail every so often. I
            only ever read: nothing gets marked as read, moved, deleted or sent.
          </Dialog.Description>

          <Field label="Your email address" htmlFor="mb-user">
            <input
              id="mb-user"
              type="email"
              autoComplete="username"
              value={username}
              onChange={(e) => {
                setUsername(e.target.value);
                setProbe(null);
              }}
            />
          </Field>

          <Field label="App password" htmlFor="mb-pass">
            <input
              id="mb-pass"
              type="password"
              autoComplete="off"
              value={password}
              onChange={(e) => {
                setPassword(e.target.value);
                setProbe(null);
              }}
            />
          </Field>
          {unsupported ? (
            <p className="danger small">{known?.help}</p>
          ) : (
            <p className="muted small">
              Not your usual password: your provider makes a separate one for apps like this.{" "}
              {known ? known.help : "Look for “app passwords” in your account's security settings."}{" "}
              It stays on this computer{sealed ? ", locked to your Windows account" : ""}. Accounts
              that only allow single sign-on (some work and school accounts) can&rsquo;t be
              connected this way &mdash; export your mail instead.
            </p>
          )}

          <details>
            <summary className="muted small">
              {known ? `Server: ${known.host}` : "Server settings"}
            </summary>
            <div className="row gap-2">
              <Field label="IMAP server" htmlFor="mb-host">
                <input
                  id="mb-host"
                  placeholder={known?.host ?? "imap.example.com"}
                  value={host}
                  onChange={(e) => {
                    setHost(e.target.value);
                    setProbe(null);
                  }}
                />
              </Field>
              <Field label="Port" htmlFor="mb-port">
                <input
                  id="mb-port"
                  type="number"
                  value={port}
                  onChange={(e) => setPort(Number(e.target.value) || 993)}
                />
              </Field>
            </div>
            <p className="muted small">Always over TLS.</p>
          </details>

          {error ? <InlineError>{error}</InlineError> : null}

          {probe ? (
            <div className="stack gap-1">
              <p>
                I&rsquo;ll read{" "}
                {probe.counts
                  .map(([f, n]) => `${f} (${n.toLocaleString()} message${n === 1 ? "" : "s"})`)
                  .join(" and ")}
                .
              </p>
              {probe.warnings.map((w) => (
                <p key={w} className="muted small">
                  {w}
                </p>
              ))}
              <label className="row gap-2">
                <input
                  type="checkbox"
                  checked={isMine}
                  onChange={(e) => setIsMine(e.target.checked)}
                />
                <span>
                  {account.username} is my own address &mdash; what&rsquo;s in its sent mail is mine
                </span>
              </label>
              <p className="muted small">
                Untick this for a mailbox you share (team@, support@), so what colleagues sent from
                it isn&rsquo;t taken as your writing.
              </p>
            </div>
          ) : null}

          <div className="row gap-2 dialog__actions">
            {probe ? (
              <Button variant="primary" onClick={connect} disabled={busy !== null}>
                {busy === "connecting" ? "Connecting…" : "Connect and start reading"}
              </Button>
            ) : (
              <Button variant="primary" onClick={look} disabled={!ready || busy !== null}>
                {busy === "checking" ? "Logging in…" : "Log in and look"}
              </Button>
            )}
            <Button variant="ghost" onClick={onClose}>
              Cancel
            </Button>
          </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
