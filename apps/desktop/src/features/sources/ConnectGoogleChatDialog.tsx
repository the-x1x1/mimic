import { useState } from "react";
import * as Dialog from "@radix-ui/react-dialog";
import { Button, InlineError } from "@mimic/ui";
import {
  useConnectGoogleChat,
  useMailSignIn,
  useSetGoogleChatSpaces,
  useSignInGoogleChatAgain,
} from "@/hooks/useSources";
import { ipc } from "@/lib/ipc";
import { toast } from "@/state/toast";

/** The command was stopped by the user: nothing to say about it. */
function wasStopped(e: unknown): boolean {
  return (e as { code?: unknown } | null)?.code === "canceled";
}

/** What reading named spaces means, said the same way wherever it's offered. */
export const SPACES_READ =
  "In named spaces I read only the threads you're in: where you've written, or someone @mentioned you — all of that thread, as Google Chat follows it for you. The rest of a busy space I leave alone.";

const GOOGLE_UNAVAILABLE =
  "This copy of Mimic isn't set up to sign in with Google yet. Settings, under Signing in with Microsoft and Google, says how to set it up.";

/**
 * Connecting Google Chat (0.10.0-alpha.79): a sign-in with Google in the
 * browser, through the registration mailboxes sign in with, asking to read
 * chats. Only a Google Workspace account's Chat can be read this way, and
 * only once the Google Chat API is on in that registration's Google Cloud
 * project: the dialog says so before anything is asked. Named spaces are
 * read only when ticked (0.10.0-alpha.92).
 */
export function ConnectGoogleChatDialog({ onClose }: { onClose: () => void }) {
  const signIns = useMailSignIn();
  const connect = useConnectGoogleChat();
  const able = signIns.data?.google === true;
  const [spaces, setSpaces] = useState(false);

  return (
    <Dialog.Root
      open
      onOpenChange={(o) => {
        if (o) return;
        if (connect.isPending) void ipc.cancelMailSignIn();
        onClose();
      }}
    >
      <Dialog.Portal>
        <Dialog.Overlay className="dialog__overlay" />
        <Dialog.Content className="dialog">
          <Dialog.Title className="dialog__title">Connect Google Chat</Dialog.Title>
          <Dialog.Description className="neutral">
            I&rsquo;ll read your Google Chat direct messages and group chats &mdash; not chats with
            apps, and named spaces only if you tick the box below &mdash; and check for new messages
            as often as I check your mail. Reading changes nothing in Google Chat.
          </Dialog.Description>
          <label className="row gap-2">
            <input
              type="checkbox"
              checked={spaces}
              disabled={connect.isPending}
              onChange={(e) => setSpaces(e.target.checked)}
            />
            <span>Also read named spaces where you&rsquo;re asked for</span>
          </label>
          <p className="muted small">{SPACES_READ}</p>
          <p className="muted small">
            You sign in with Google in your browser, with a Google Workspace account (a work or
            school address on Google); a personal Gmail account has no Google Chat API. I ask to
            read your chats, to send a message you approve, and who you are.
          </p>
          <p className="muted small">
            Once, in the Google Cloud project Mimic signs in through: turn on the Google Chat API,
            and on its Configuration page give the app a name (say, Mimic), an avatar address and a
            description, with interactive features off.
          </p>
          {signIns.data && !able ? <p className="neutral">{GOOGLE_UNAVAILABLE}</p> : null}
          {connect.isPending ? (
            <p className="neutral" role="status">
              Finish signing in in your browser. I&rsquo;ll carry on when you&rsquo;re done.
            </p>
          ) : null}
          {connect.isError && !wasStopped(connect.error) ? (
            <InlineError>{(connect.error as Error).message}</InlineError>
          ) : null}
          <div className="row gap-2 dialog__actions">
            {connect.isPending ? (
              <Button variant="ghost" onClick={() => void ipc.cancelMailSignIn()}>
                Stop signing in
              </Button>
            ) : (
              <Button
                variant="primary"
                disabled={!able}
                onClick={() =>
                  connect.mutate(spaces, {
                    onSuccess: (source) => {
                      toast.success("Connected.", `I'm reading ${source.name} now.`);
                      onClose();
                    },
                  })
                }
              >
                Sign in with Google
              </Button>
            )}
            <Button
              variant="ghost"
              onClick={() => {
                if (connect.isPending) void ipc.cancelMailSignIn();
                onClose();
              }}
            >
              Cancel
            </Button>
          </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}

/**
 * Whether a connected Google Chat account's named spaces are read
 * (0.10.0-alpha.92): off for one connected before they could be.
 */
export function GoogleChatSpaces({ sourceId, on }: { sourceId: string; on: boolean }) {
  const set = useSetGoogleChatSpaces();
  return (
    <div className="stack gap-1">
      <label className="row gap-2 small">
        <input
          type="checkbox"
          checked={on}
          disabled={set.isPending}
          onChange={(e) => set.mutate({ sourceId, on: e.target.checked })}
        />
        <span>Read named spaces where you&rsquo;re asked for</span>
      </label>
      <p className="muted small">
        {on
          ? SPACES_READ
          : "Off: I read direct messages and group chats only. What I read from spaces before stays."}
      </p>
      {set.isError ? <InlineError>{(set.error as Error).message}</InlineError> : null}
    </div>
  );
}

/**
 * A connected Google Chat account, signed in again as the same account —
 * when Google asked for it — keeping everything read through it.
 */
export function GoogleChatSignInAgain({ sourceId, name }: { sourceId: string; name: string }) {
  const again = useSignInGoogleChatAgain();
  return (
    <div className="stack gap-1">
      {again.isPending ? (
        <Button size="sm" variant="ghost" onClick={() => void ipc.cancelMailSignIn()}>
          Stop signing in
        </Button>
      ) : (
        <Button
          size="sm"
          variant="ghost"
          onClick={() =>
            again.mutate(sourceId, {
              onSuccess: () => toast.success("Signed in again", `I'll check ${name} now.`),
            })
          }
        >
          Sign in again
        </Button>
      )}
      {again.isError && !wasStopped(again.error) ? (
        <InlineError>{(again.error as Error).message}</InlineError>
      ) : null}
    </div>
  );
}
