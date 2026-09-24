import { useState } from "react";
import * as Dialog from "@radix-ui/react-dialog";
import { useQueryClient } from "@tanstack/react-query";
import { Button, Field, InlineError } from "@mimic/ui";
import {
  MICROSOFT_UNAVAILABLE,
  guessMailHost,
  type ImapAccount,
  type ImapProbe,
} from "@mimic/contracts";
import { ipc } from "@/lib/ipc";
import { qk } from "@/app/queryClient";
import { toast } from "@/state/toast";
import { useProviderState } from "@/hooks/useCompose";
import { useMailSignIn } from "@/hooks/useSources";

/** The command was stopped by the user: nothing to say about it. */
function wasStopped(e: unknown): boolean {
  return (e as { code?: unknown } | null)?.code === "canceled";
}

/**
 * Connecting a mailbox is: say which one, sign in once to look around, see
 * what will be read, then decide. The look-around step exists so a missing
 * sent folder — which would leave Mimic nothing of yours to learn from — is
 * known before anything is imported, not discovered a week later.
 *
 * Most providers want an app password, and the copy says so first, because
 * the usual one fails on every one of them and the error from the server
 * does not say why. Outlook.com, Hotmail and Microsoft 365 take no password
 * at all: those sign in with Microsoft in the browser, when this copy of
 * Mimic was built able to — and when it wasn't, the dialog says so and offers
 * nothing that would fail.
 */
export function ConnectMailboxDialog({ onClose }: { onClose: () => void }) {
  const qc = useQueryClient();
  const [username, setUsername] = useState("");
  const [host, setHost] = useState("");
  const [port, setPort] = useState(993);
  const [password, setPassword] = useState("");
  const [workMicrosoft, setWorkMicrosoft] = useState(false);
  const [probe, setProbe] = useState<ImapProbe | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState<"checking" | "connecting" | "signing-in" | null>(null);
  const [isMine, setIsMine] = useState(true);
  // Said only when the store reports it: until then, and on a build that
  // does not seal, the note claims nothing it cannot back.
  const protection = useProviderState().data?.credentials.protection;
  const signInAvailable = useMailSignIn().data;
  const canSignIn = signInAvailable === true;

  const known = guessMailHost(username);
  const microsoft = known?.signIn === "microsoft" || workMicrosoft;
  const account: ImapAccount = {
    host: host.trim() || known?.host || "",
    port: host.trim() ? port : (known?.port ?? port),
    username: username.trim(),
    security: "tls",
  };
  const ready = account.host !== "" && account.username !== "" && password !== "";
  const locked =
    protection === "account"
      ? ", locked to your Windows account"
      : protection === "keychain"
        ? ", locked with a key in your Keychain"
        : "";
  const kept = `It stays on this computer${locked}.`;

  /** A sign-in brought back for another address, or abandoned, is forgotten. */
  function forgetSignIn() {
    if (microsoft && (busy === "signing-in" || probe)) void ipc.cancelMailSignIn();
  }

  function close() {
    forgetSignIn();
    onClose();
  }

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

  async function signIn() {
    setError(null);
    setProbe(null);
    setBusy("signing-in");
    try {
      setProbe(await ipc.signInToMailbox(account.username));
    } catch (e) {
      if (!wasStopped(e)) setError((e as Error).message);
    } finally {
      setBusy(null);
    }
  }

  async function connect() {
    setError(null);
    setBusy("connecting");
    try {
      if (microsoft) {
        await ipc.connectSignedInMailbox(account.username, isMine);
      } else {
        await ipc.connectMailbox(account, password, isMine);
      }
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
    <Dialog.Root open onOpenChange={(o) => !o && close()}>
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
              disabled={busy === "signing-in"}
              onChange={(e) => {
                forgetSignIn();
                setUsername(e.target.value);
                setProbe(null);
              }}
            />
          </Field>

          {microsoft ? (
            canSignIn ? (
              <p className="muted small">
                {known?.signIn === "microsoft"
                  ? known.help
                  : "Microsoft accounts — Outlook, Hotmail and Microsoft 365 — sign in with Microsoft."}{" "}
                I never see your password; what lets me stay signed in is kept instead. {kept} If
                your organisation hasn&rsquo;t allowed apps like this, Microsoft will say so when
                you sign in.
              </p>
            ) : signInAvailable === false ? (
              <p className="danger small">{MICROSOFT_UNAVAILABLE}</p>
            ) : null
          ) : (
            <>
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
              <p className="muted small">
                Not your usual password: your provider makes a separate one for apps like this.{" "}
                {known
                  ? known.help
                  : "Look for “app passwords” in your account's security settings."}{" "}
                {kept}{" "}
                {canSignIn
                  ? "A Microsoft 365 work or school account, or any other Microsoft address, takes no password at all: say so under Server settings, and sign in with Microsoft."
                  : "Microsoft 365 accounts, and work accounts that allow only single sign-on, can't be connected with a password — export your mail instead."}
              </p>
            </>
          )}

          <details>
            <summary className="muted small">
              {known ? `Server: ${known.host}` : "Server settings"}
            </summary>
            {known?.signIn === "microsoft" ? null : (
              <>
                {canSignIn ? (
                  <label className="row gap-2">
                    <input
                      type="checkbox"
                      checked={workMicrosoft}
                      disabled={busy !== null}
                      onChange={(e) => {
                        forgetSignIn();
                        setWorkMicrosoft(e.target.checked);
                        setProbe(null);
                      }}
                    />
                    <span>
                      This is a Microsoft account (Outlook, Hotmail or Microsoft 365): sign in with
                      Microsoft
                    </span>
                  </label>
                ) : null}
                {workMicrosoft ? null : (
                  <>
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
                  </>
                )}
              </>
            )}
          </details>

          {busy === "signing-in" ? (
            <p className="neutral">
              Finish signing in in your browser. I&rsquo;ll carry on when you&rsquo;re done.
            </p>
          ) : null}

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
              {isMine ? (
                <p className="muted small">
                  Mail from it I&rsquo;ve already read &mdash; in an export of this mailbox, say
                  &mdash; becomes yours too if I filed it under someone whose every address is yours
                  and for whom you&rsquo;ve set no relationship, notes or preferences. If I filed it
                  under anyone else, Settings lists them under You, to say whether they&rsquo;re
                  you.
                </p>
              ) : null}
            </div>
          ) : null}

          <div className="row gap-2 dialog__actions">
            {probe ? (
              <Button variant="primary" onClick={connect} disabled={busy !== null}>
                {busy === "connecting" ? "Connecting…" : "Connect and start reading"}
              </Button>
            ) : microsoft ? (
              busy === "signing-in" ? (
                <Button variant="ghost" onClick={() => void ipc.cancelMailSignIn()}>
                  Stop signing in
                </Button>
              ) : (
                <Button
                  variant="primary"
                  onClick={signIn}
                  disabled={!canSignIn || !account.username.includes("@") || busy !== null}
                >
                  Sign in with Microsoft
                </Button>
              )
            ) : (
              <Button variant="primary" onClick={look} disabled={!ready || busy !== null}>
                {busy === "checking" ? "Logging in…" : "Log in and look"}
              </Button>
            )}
            <Button variant="ghost" onClick={close}>
              Cancel
            </Button>
          </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
