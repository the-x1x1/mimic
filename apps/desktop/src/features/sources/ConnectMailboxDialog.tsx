import { useState } from "react";
import * as Dialog from "@radix-ui/react-dialog";
import { useQueryClient } from "@tanstack/react-query";
import { Button, Field, InlineError } from "@mimic/ui";
import {
  MICROSOFT_UNAVAILABLE,
  SIGN_IN_NAMES,
  guessMailHost,
  mailDomainOf,
  mailHostFromExchangers,
  type ImapAccount,
  type ImapProbe,
  type MailServers,
  type SignInWith,
} from "@mimic/contracts";
import { ipc } from "@/lib/ipc";
import { qk } from "@/app/queryClient";
import { toast } from "@/state/toast";
import { useProviderState } from "@/hooks/useCompose";
import { useMailExchangers, useMailSignIn } from "@/hooks/useSources";

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
 * nothing that would fail. Gmail and Google Workspace sign in with Google the
 * same way when this copy can, and an app password still works for them.
 *
 * An address at a domain of its own — a business's, a family's — says
 * nothing about whose servers its mail is on, so when no provider is known
 * for it, leaving the address field asks this computer's resolver who
 * handles its mail (`mail_exchangers`), and Google Workspace, Microsoft 365
 * and a few others are recognised from that (`mailHostFromExchangers`) and
 * treated as if the address were theirs.
 */
export function ConnectMailboxDialog({ onClose }: { onClose: () => void }) {
  const qc = useQueryClient();
  const [username, setUsername] = useState("");
  const [host, setHost] = useState("");
  const [port, setPort] = useState(993);
  const [password, setPassword] = useState("");
  // Ticked under Server settings: an address at the user's own domain whose
  // mail is Microsoft 365's or Google Workspace's.
  const [workAccount, setWorkAccount] = useState<SignInWith | null>(null);
  // Gmail with an app password, though it could sign in with Google.
  const [withPassword, setWithPassword] = useState(false);
  const [probe, setProbe] = useState<ImapProbe | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState<"checking" | "connecting" | "signing-in" | null>(null);
  const [isMine, setIsMine] = useState(true);
  // What reaching the servers found, signed in to neither (0.10.0-alpha.89),
  // and for which server, port and address: shown only while they are still
  // the ones in the form, so an edit — or an answer that comes back after
  // one — never leaves another server's answer showing (0.10.0-alpha.93).
  const [reachedFor, setReachedFor] = useState<{ key: string; found: MailServers } | null>(null);
  const [reaching, setReaching] = useState(false);
  // Said only when the store reports it: until then, and on a build that
  // does not seal, the note claims nothing it cannot back.
  const protection = useProviderState().data?.credentials.protection;
  const signIns = useMailSignIn().data;

  const domain = mailDomainOf(username);
  const listed = guessMailHost(username);
  // The domain asked about on leaving the address field, while it is still
  // the address's.
  const [lookFor, setLookFor] = useState<string | null>(null);
  const asking = !listed && lookFor !== null && lookFor === domain ? lookFor : null;
  const exchangers = useMailExchangers(asking);
  const found = asking && exchangers.data ? mailHostFromExchangers(exchangers.data) : null;
  const known = listed ?? found;
  // How this mailbox will sign in: a known provider's way when it takes
  // nothing else, or when this copy can and the user didn't choose a
  // password; for any other address, the way ticked under Server settings
  // (offered only for an address no provider is known for).
  const signingInWith: SignInWith | null = known?.signIn
    ? known.noPassword || (signIns?.[known.signIn] === true && !withPassword)
      ? known.signIn
      : null
    : workAccount;
  const canSignIn = signingInWith !== null && signIns?.[signingInWith] === true;
  const provider = signingInWith ? SIGN_IN_NAMES[signingInWith] : null;
  const account: ImapAccount = {
    host: host.trim() || known?.host || "",
    port: host.trim() ? port : (known?.port ?? port),
    username: username.trim(),
    security: "tls",
  };
  const ready = account.host !== "" && account.username !== "" && password !== "";
  const serversKey = `${account.host}\n${account.port}\n${account.username}`;
  const servers = reachedFor?.key === serversKey ? reachedFor.found : null;
  const locked =
    protection === "account"
      ? ", locked to your Windows account"
      : protection === "keychain"
        ? ", locked with a key in your Keychain"
        : "";
  const kept = `It stays on this computer${locked}.`;

  /** A sign-in brought back for another address, or abandoned, is forgotten. */
  function forgetSignIn() {
    if (signingInWith && (busy === "signing-in" || probe)) void ipc.cancelMailSignIn();
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

  async function signIn(signInWith: SignInWith) {
    setError(null);
    setProbe(null);
    setBusy("signing-in");
    try {
      setProbe(await ipc.signInToMailbox(account.username, signInWith));
    } catch (e) {
      if (!wasStopped(e)) setError((e as Error).message);
    } finally {
      setBusy(null);
    }
  }

  /** Reach the servers as connecting would, sending no password; say what each said. */
  async function checkServers() {
    setError(null);
    setReachedFor(null);
    setReaching(true);
    const key = serversKey;
    try {
      const found = await ipc.reachMailServers(account.host, account.port, account.username);
      setReachedFor({ key, found });
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setReaching(false);
    }
  }

  async function connect() {
    setError(null);
    setBusy("connecting");
    try {
      if (signingInWith) {
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
            I&rsquo;ll read your inbox and your sent mail, and check for new mail every so often.
            Reading changes nothing: nothing gets marked as read, moved or deleted. I send only a
            reply you&rsquo;ve said can go.
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
              onBlur={(e) => setLookFor(mailDomainOf(e.target.value))}
            />
          </Field>
          {asking && exchangers.isFetching ? (
            <p className="muted small" role="status">
              Looking up who handles mail for {asking}&hellip;
            </p>
          ) : found ? (
            <p className="muted small" role="status">
              Mail for {asking} is handled by {found.name}.
            </p>
          ) : null}

          {signingInWith ? (
            canSignIn ? (
              <div className="stack gap-1">
                <p className="muted small">
                  {signingInWith === "microsoft"
                    ? known?.signIn === "microsoft"
                      ? known.help
                      : "Microsoft accounts — Outlook, Hotmail and Microsoft 365 — sign in with Microsoft."
                    : listed?.signIn === "google"
                      ? "Gmail signs in with Google in your browser: no app password needed."
                      : "Google accounts — Gmail and Google Workspace — sign in with Google."}{" "}
                  I never see your password; what lets me stay signed in is kept instead. {kept}{" "}
                  {signingInWith === "microsoft"
                    ? "If your organisation hasn’t allowed apps like this, Microsoft will say so when you sign in."
                    : "Google asks you to allow full access to your mail — the only permission it offers mail programs — and may warn that it hasn’t checked Mimic. I use it to read your mail and send the replies you let go."}
                </p>
                {known?.signIn === "google" ? (
                  <div className="row">
                    <Button
                      size="sm"
                      variant="ghost"
                      disabled={busy !== null}
                      onClick={() => {
                        forgetSignIn();
                        setWithPassword(true);
                        setProbe(null);
                      }}
                    >
                      Use an app password instead
                    </Button>
                  </div>
                ) : null}
              </div>
            ) : signIns && signingInWith === "microsoft" && !signIns.microsoft ? (
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
                {known
                  ? null
                  : signIns?.microsoft
                    ? "A Microsoft 365 work or school account, or any other Microsoft address, takes no password at all: say so under Server settings, and sign in with Microsoft."
                    : "Microsoft 365 accounts, and work accounts that allow only single sign-on, can't be connected with a password — export your mail instead."}{" "}
                {!known && signIns?.google
                  ? "A Google Workspace account can sign in with Google instead: say so under Server settings."
                  : null}
                {known?.signIn === "google" && signIns && !signIns.google
                  ? "Or sign in with Google instead, with no app password, once this copy is set up to: Settings, under Signing in with Microsoft and Google, says how."
                  : null}
              </p>
              {known?.signIn === "google" && signIns?.google ? (
                <div className="row">
                  <Button
                    size="sm"
                    variant="ghost"
                    disabled={busy !== null}
                    onClick={() => {
                      setWithPassword(false);
                      setProbe(null);
                    }}
                  >
                    Sign in with Google instead
                  </Button>
                </div>
              ) : null}
            </>
          )}

          <details>
            <summary className="muted small">
              {known ? `Server: ${known.host}` : "Server settings"}
            </summary>
            {known?.signIn ? null : (
              <>
                {(["microsoft", "google"] as const).map((w) =>
                  signIns?.[w] ? (
                    <label key={w} className="row gap-2">
                      <input
                        type="checkbox"
                        checked={workAccount === w}
                        disabled={busy !== null}
                        onChange={(e) => {
                          forgetSignIn();
                          setWorkAccount(e.target.checked ? w : null);
                          setProbe(null);
                        }}
                      />
                      <span>
                        {w === "microsoft"
                          ? "This is a Microsoft account (Outlook, Hotmail or Microsoft 365): sign in with Microsoft"
                          : "This is a Google account (Gmail or Google Workspace): sign in with Google"}
                      </span>
                    </label>
                  ) : null,
                )}
                {workAccount ? null : (
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
                          onChange={(e) => {
                            setPort(Number(e.target.value) || 993);
                          }}
                        />
                      </Field>
                    </div>
                    <p className="muted small">Always over TLS.</p>
                  </>
                )}
              </>
            )}
            {account.host ? (
              <div className="stack gap-1">
                <div className="row gap-2">
                  <Button
                    size="sm"
                    variant="ghost"
                    disabled={reaching || busy !== null}
                    onClick={() => void checkServers()}
                  >
                    {reaching ? "Checking the server…" : "Check the server (no password sent)"}
                  </Button>
                </div>
                {servers ? (
                  <ul className="small" aria-label="What the servers said">
                    {[servers.reading, ...(servers.sending ? [servers.sending] : [])].map((s) => (
                      <li key={`${s.host}:${s.port}`}>{`${s.host}:${s.port} \u2014 ${s.said}`}</li>
                    ))}
                  </ul>
                ) : null}
              </div>
            ) : null}
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
            ) : signingInWith ? (
              busy === "signing-in" ? (
                <Button variant="ghost" onClick={() => void ipc.cancelMailSignIn()}>
                  Stop signing in
                </Button>
              ) : (
                <Button
                  variant="primary"
                  onClick={() => void signIn(signingInWith)}
                  disabled={!canSignIn || !account.username.includes("@") || busy !== null}
                >
                  Sign in with {provider}
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
