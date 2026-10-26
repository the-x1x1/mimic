import { useState } from "react";
import * as Dialog from "@radix-ui/react-dialog";
import { Button, Field, InlineError } from "@mimic/ui";
import { SLACK_MANIFEST, isSlackUserToken } from "@mimic/contracts";
import { useConnectSlack, useSetSlackToken } from "@/hooks/useSources";
import { toast } from "@/state/toast";

/**
 * Connecting Slack (0.10.0-alpha.64). Slack lets an app read someone's
 * direct messages only with a token that person made, from an app of their
 * own installed to their workspace: so the steps make one from a manifest,
 * and the token it gives is pasted here. It is tried before anything is
 * kept, and kept with the other saved sign-ins, never in the database.
 */
export function ConnectSlackDialog({ onClose }: { onClose: () => void }) {
  const [token, setToken] = useState("");
  const [copied, setCopied] = useState(false);
  const connect = useConnectSlack();
  const usable = isSlackUserToken(token);

  return (
    <Dialog.Root open onOpenChange={(o) => !o && onClose()}>
      <Dialog.Portal>
        <Dialog.Overlay className="dialog__overlay" />
        <Dialog.Content className="dialog dialog--wide">
          <Dialog.Title className="dialog__title">Connect Slack</Dialog.Title>
          <Dialog.Description className="neutral">
            I&rsquo;ll read your direct messages and group messages in one workspace &mdash; not
            channels &mdash; and check for new ones as often as I check your mail. Reading changes
            nothing in Slack.
          </Dialog.Description>

          <ol className="small stack gap-1">
            <li>
              At api.slack.com/apps, choose <strong>Create New App</strong>, then{" "}
              <strong>From a manifest</strong>, and pick your workspace.
            </li>
            <li>
              Paste this manifest and create the app. It asks only to read your direct messages, who
              people are, and to write as you, for replies you approve.{" "}
              <Button
                size="sm"
                variant="ghost"
                onClick={() => {
                  void navigator.clipboard
                    .writeText(SLACK_MANIFEST)
                    .then(() => setCopied(true))
                    .catch(() => setCopied(false));
                }}
              >
                {copied ? "Copied" : "Copy the manifest"}
              </Button>
              <pre className="mono small" aria-label="Slack app manifest">
                {SLACK_MANIFEST}
              </pre>
            </li>
            <li>
              Under <strong>Install App</strong>, install it to your workspace and allow it. Your
              workspace&rsquo;s admins may have to approve it first.
            </li>
            <li>
              Copy the <strong>User OAuth Token</strong> &mdash; it starts with xoxp- &mdash; and
              paste it here.
            </li>
          </ol>

          <Field label="User OAuth Token" htmlFor="slack-token">
            <input
              id="slack-token"
              type="password"
              autoComplete="off"
              value={token}
              onChange={(e) => {
                setToken(e.target.value);
                connect.reset();
              }}
            />
          </Field>
          {token.trim() !== "" && !usable ? (
            <p className="muted small" role="status">
              That isn&rsquo;t a user token: the one I need starts with xoxp-. A bot token (xoxb-)
              can&rsquo;t read your direct messages.
            </p>
          ) : null}
          <p className="muted small">
            I&rsquo;ll try it first, and keep it only if it works, sealed to your Windows account
            with your other saved sign-ins. Removing the workspace forgets it.
          </p>
          {connect.isError ? <InlineError>{(connect.error as Error).message}</InlineError> : null}

          <div className="row gap-2 dialog__actions">
            <Button
              variant="primary"
              disabled={!usable || connect.isPending}
              onClick={() =>
                connect.mutate(token, {
                  onSuccess: (source) => {
                    toast.success("Connected.", `I'm reading ${source.name} now.`);
                    onClose();
                  },
                })
              }
            >
              {connect.isPending ? "Trying it…" : "Connect and start reading"}
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

/**
 * A connected workspace's token, given again — when Slack turned the old one
 * down, or the app was reinstalled with another scope — keeping everything
 * read through it.
 */
export function SlackToken({ sourceId, name }: { sourceId: string; name: string }) {
  const [open, setOpen] = useState(false);
  const [token, setToken] = useState("");
  const save = useSetSlackToken();

  if (!open) {
    return (
      <Button size="sm" variant="ghost" onClick={() => setOpen(true)}>
        New token
      </Button>
    );
  }

  return (
    <form
      className="stack gap-1"
      onSubmit={(e) => {
        e.preventDefault();
        save.mutate(
          { sourceId, token },
          {
            onSuccess: () => {
              setToken("");
              setOpen(false);
              toast.success("Token saved", `I'll check ${name} now.`);
            },
          },
        );
      }}
    >
      <div className="row gap-1">
        <input
          type="password"
          autoComplete="off"
          aria-label={`New Slack token for ${name}`}
          value={token}
          onChange={(e) => {
            setToken(e.target.value);
            save.reset();
          }}
        />
        <Button size="sm" type="submit" disabled={!isSlackUserToken(token) || save.isPending}>
          {save.isPending ? "Trying it…" : "Save"}
        </Button>
        <Button
          size="sm"
          variant="ghost"
          type="button"
          onClick={() => {
            setToken("");
            setOpen(false);
            save.reset();
          }}
        >
          Cancel
        </Button>
      </div>
      <p className="muted small">
        The User OAuth Token from your Slack app, for the same account. I&rsquo;ll try it first.
        Everything already read stays.
      </p>
      {save.isError ? <InlineError>{(save.error as Error).message}</InlineError> : null}
    </form>
  );
}
