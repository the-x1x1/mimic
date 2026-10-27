import { useState } from "react";
import * as Dialog from "@radix-ui/react-dialog";
import { Button, InlineError } from "@mimic/ui";
import { useTelegramApp, useTelegramSignIn } from "@/hooks/useSources";
import { ipc } from "@/lib/ipc";
import { toast } from "@/state/toast";

type Stage =
  { at: "start" } | { at: "code" } | { at: "password"; hint: string | null; wrong: boolean };

/**
 * Connecting Telegram (0.10.0-alpha.74): the app the user registers at
 * my.telegram.org (once), then their phone number, the code Telegram sends
 * and, if the account has one, its password — which goes to Telegram and is
 * kept nowhere. Closing it stops the sign-in.
 */
export function ConnectTelegramDialog({ onClose }: { onClose: () => void }) {
  const app = useTelegramApp(true);
  const { start, code, password } = useTelegramSignIn();
  const [stage, setStage] = useState<Stage>({ at: "start" });
  const [apiId, setApiId] = useState("");
  const [apiHash, setApiHash] = useState("");
  const [phone, setPhone] = useState("");
  const [codeText, setCodeText] = useState("");
  const [passwordText, setPasswordText] = useState("");
  const saved = app.data?.saved === true;
  const [newApp, setNewApp] = useState(false);
  const askApp = !saved || newApp;
  const busy = start.isPending || code.isPending || password.isPending;

  function close() {
    if (stage.at !== "start" || start.isPending) void ipc.cancelTelegramSignIn();
    onClose();
  }

  function done(step: {
    step: string;
    hint: string | null;
    wrong: boolean;
    source: { name: string } | null;
  }) {
    if (step.step === "done") {
      toast.success("Connected.", `I'm reading ${step.source?.name ?? "your chats"} now.`);
      onClose();
    } else if (step.step === "password") {
      setPasswordText("");
      setStage({ at: "password", hint: step.hint, wrong: step.wrong });
    } else {
      setStage({ at: "code" });
    }
  }

  const error = start.error ?? code.error ?? password.error;

  return (
    <Dialog.Root
      open
      onOpenChange={(o) => {
        if (!o) close();
      }}
    >
      <Dialog.Portal>
        <Dialog.Overlay className="dialog__overlay" />
        <Dialog.Content className="dialog">
          <Dialog.Title className="dialog__title">Connect Telegram</Dialog.Title>
          <Dialog.Description className="neutral">
            I&rsquo;ll read your one-to-one chats and private groups &mdash; not channels, bots or
            public groups &mdash; and check for new messages as often as I check your mail. Reading
            changes nothing in Telegram; a chat you exported before joins up with what I read.
          </Dialog.Description>

          {stage.at === "start" ? (
            <form
              className="stack gap-2"
              aria-label="Telegram sign-in"
              onSubmit={(e) => {
                e.preventDefault();
                start.reset();
                start.mutate(
                  { phone, apiId: askApp ? apiId : null, apiHash: askApp ? apiHash : null },
                  { onSuccess: done },
                );
              }}
            >
              {askApp ? (
                <>
                  <ol className="stack gap-1 small">
                    <li>
                      Sign in at <span className="mono">my.telegram.org</span> with your phone
                      number.
                    </li>
                    <li>
                      Open <strong>API development tools</strong> and create an app: any title (say,
                      Mimic), platform Desktop.
                    </li>
                    <li>
                      Copy its App api_id and App api_hash here. They&rsquo;re yours: keep them to
                      yourself.
                    </li>
                  </ol>
                  <label className="stack gap-1">
                    <span className="muted small">App api_id</span>
                    <input
                      value={apiId}
                      inputMode="numeric"
                      onChange={(e) => setApiId(e.target.value)}
                    />
                  </label>
                  <label className="stack gap-1">
                    <span className="muted small">App api_hash</span>
                    <input
                      value={apiHash}
                      autoComplete="off"
                      spellCheck={false}
                      onChange={(e) => setApiHash(e.target.value)}
                    />
                  </label>
                </>
              ) : (
                <p className="muted small">
                  Through the app you registered (api_id {app.data?.apiId ?? ""}).{" "}
                  <button type="button" className="linkish" onClick={() => setNewApp(true)}>
                    Use another
                  </button>
                </p>
              )}
              <label className="stack gap-1">
                <span className="muted small">Your phone number, with its country code</span>
                <input
                  value={phone}
                  type="tel"
                  autoComplete="tel"
                  placeholder="+1 808 555 0123"
                  onChange={(e) => setPhone(e.target.value)}
                />
              </label>
              <p className="muted small">
                Telegram sends you a code, in Telegram itself or by text. The app&rsquo;s id and
                hash and what the sign-in leaves are kept sealed on this computer, never in my
                database.
              </p>
              <div className="row gap-2 dialog__actions">
                <Button variant="primary" type="submit" disabled={busy || phone.trim() === ""}>
                  {start.isPending ? "Asking Telegram…" : "Send me a code"}
                </Button>
                <Button variant="ghost" type="button" onClick={close}>
                  Cancel
                </Button>
              </div>
            </form>
          ) : stage.at === "code" ? (
            <form
              className="stack gap-2"
              aria-label="Telegram code"
              onSubmit={(e) => {
                e.preventDefault();
                code.reset();
                code.mutate(codeText, { onSuccess: done });
              }}
            >
              <label className="stack gap-1">
                <span className="muted small">The code Telegram sent you</span>
                <input
                  value={codeText}
                  inputMode="numeric"
                  autoComplete="one-time-code"
                  onChange={(e) => setCodeText(e.target.value)}
                />
              </label>
              <div className="row gap-2 dialog__actions">
                <Button variant="primary" type="submit" disabled={busy || codeText.trim() === ""}>
                  {code.isPending ? "Signing in…" : "Sign in"}
                </Button>
                <Button variant="ghost" type="button" onClick={close}>
                  Cancel
                </Button>
              </div>
            </form>
          ) : (
            <form
              className="stack gap-2"
              aria-label="Telegram password"
              onSubmit={(e) => {
                e.preventDefault();
                password.reset();
                password.mutate(passwordText, {
                  onSuccess: done,
                  onSettled: () => setPasswordText(""),
                });
              }}
            >
              <p className="neutral">
                Your account has a password too
                {stage.hint ? ` (its hint: ${stage.hint})` : ""}. It goes to Telegram to sign in,
                and I keep it nowhere.
              </p>
              {stage.wrong ? (
                <p className="danger small">That password isn&rsquo;t right.</p>
              ) : null}
              <label className="stack gap-1">
                <span className="muted small">Password</span>
                <input
                  value={passwordText}
                  type="password"
                  autoComplete="current-password"
                  onChange={(e) => setPasswordText(e.target.value)}
                />
              </label>
              <div className="row gap-2 dialog__actions">
                <Button variant="primary" type="submit" disabled={busy || passwordText === ""}>
                  {password.isPending ? "Signing in…" : "Sign in"}
                </Button>
                <Button variant="ghost" type="button" onClick={close}>
                  Cancel
                </Button>
              </div>
            </form>
          )}
          {error ? <InlineError>{error.message}</InlineError> : null}
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
