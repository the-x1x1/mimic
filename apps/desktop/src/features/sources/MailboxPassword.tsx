import { useState } from "react";
import { Button, InlineError } from "@mimic/ui";
import { useMailSignIn, useSetMailboxPassword, useSignInMailboxAgain } from "@/hooks/useSources";
import { ipc } from "@/lib/ipc";
import { toast } from "@/state/toast";

/**
 * A connected mailbox's app password, given again — when the provider's was
 * replaced, or the saved one can't be unlocked on this Windows account —
 * without removing the mailbox and everything read from it.
 */
export function MailboxPassword({ sourceId, name }: { sourceId: string; name: string }) {
  const [open, setOpen] = useState(false);
  const [password, setPassword] = useState("");
  const save = useSetMailboxPassword();

  if (!open) {
    return (
      <Button size="sm" variant="ghost" onClick={() => setOpen(true)}>
        New password
      </Button>
    );
  }

  return (
    <form
      className="stack gap-1"
      onSubmit={(e) => {
        e.preventDefault();
        save.mutate(
          { sourceId, password },
          {
            onSuccess: () => {
              setPassword("");
              setOpen(false);
              toast.success("Password saved", `I'll check ${name} now.`);
            },
          },
        );
      }}
    >
      <div className="row gap-1">
        <input
          type="password"
          autoComplete="off"
          aria-label={`New app password for ${name}`}
          value={password}
          onChange={(e) => {
            setPassword(e.target.value);
            save.reset();
          }}
        />
        <Button size="sm" type="submit" disabled={password === "" || save.isPending}>
          {save.isPending ? "Checking…" : "Save"}
        </Button>
        <Button
          size="sm"
          variant="ghost"
          type="button"
          onClick={() => {
            setPassword("");
            setOpen(false);
            save.reset();
          }}
        >
          Cancel
        </Button>
      </div>
      <p className="muted small">
        I&rsquo;ll log in with it first, and keep it only if that works. Everything already read
        from this mailbox stays.
      </p>
      {save.isError ? <InlineError>{(save.error as Error).message}</InlineError> : null}
    </form>
  );
}

/**
 * A connected Microsoft mailbox, signed in again — when Microsoft asks for it,
 * or the saved sign-in can't be unlocked on this Windows account — without
 * removing it and everything read from it. The new sign-in is tried on the
 * mailbox first, and kept only if it opens it.
 */
export function MailboxSignInAgain({ sourceId, name }: { sourceId: string; name: string }) {
  const again = useSignInMailboxAgain();
  const available = useMailSignIn().data === true;
  const stopped = (again.error as { code?: unknown } | null)?.code === "canceled";

  if (!available) return null;
  return (
    <div className="stack gap-1">
      {again.isPending ? (
        <div className="row gap-1">
          <span className="muted small">Finish signing in in your browser.</span>
          <Button size="sm" variant="ghost" onClick={() => void ipc.cancelMailSignIn()}>
            Stop
          </Button>
        </div>
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
      {again.isError && !stopped ? (
        <InlineError>{(again.error as Error).message}</InlineError>
      ) : null}
    </div>
  );
}
