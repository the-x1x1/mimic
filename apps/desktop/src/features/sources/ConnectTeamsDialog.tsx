import * as Dialog from "@radix-ui/react-dialog";
import { Button, InlineError } from "@mimic/ui";
import { MICROSOFT_UNAVAILABLE } from "@mimic/contracts";
import { useConnectTeams, useMailSignIn, useSignInTeamsAgain } from "@/hooks/useSources";
import { ipc } from "@/lib/ipc";
import { toast } from "@/state/toast";

/** The command was stopped by the user: nothing to say about it. */
function wasStopped(e: unknown): boolean {
  return (e as { code?: unknown } | null)?.code === "canceled";
}

/**
 * Connecting Microsoft Teams (0.10.0-alpha.67): a sign-in with Microsoft in
 * the browser, through the registration mailboxes sign in with, asking to
 * read chats. Only a work or school account's Teams can be read this way,
 * and the dialog says so before anything is asked.
 */
export function ConnectTeamsDialog({ onClose }: { onClose: () => void }) {
  const signIns = useMailSignIn();
  const connect = useConnectTeams();
  const able = signIns.data?.microsoft === true;

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
          <Dialog.Title className="dialog__title">Connect Teams</Dialog.Title>
          <Dialog.Description className="neutral">
            I&rsquo;ll read your Teams chats &mdash; one-to-one and group, not meetings or channels
            &mdash; and check for new messages as often as I check your mail. Reading changes
            nothing in Teams.
          </Dialog.Description>
          <p className="muted small">
            You sign in with Microsoft in your browser, with a work or school account; a personal
            Microsoft account&rsquo;s Teams can&rsquo;t be read this way. I ask to read your chats,
            to send a message you approve, and who you are. Your organisation may have to allow it
            first.
          </p>
          {signIns.data && !able ? <p className="neutral">{MICROSOFT_UNAVAILABLE}</p> : null}
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
                  connect.mutate(undefined, {
                    onSuccess: (source) => {
                      toast.success("Connected.", `I'm reading ${source.name} now.`);
                      onClose();
                    },
                  })
                }
              >
                Sign in with Microsoft
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
 * A connected Teams account, signed in again as the same account — when
 * Microsoft asked for it — keeping everything read through it.
 */
export function TeamsSignInAgain({ sourceId, name }: { sourceId: string; name: string }) {
  const again = useSignInTeamsAgain();
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
