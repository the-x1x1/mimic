import { Button, InlineError } from "@mimic/ui";
import { describeHeld, type OutboxEntry } from "@mimic/contracts";
import { useHeldReply } from "@/hooks/useSending";

/**
 * A reply Mimic is holding to send by itself, under the message it answers:
 * when it will go and to whom, with the two things the user can do about it
 * — let it go now, or stop it, after which the draft waits for them like any
 * other. Every rule it was held under is checked again when it goes.
 */
export function HeldReply({ entry }: { entry: OutboxEntry }) {
  const { stop, now } = useHeldReply();
  const busy = stop.isPending || now.isPending;
  const error = stop.error ?? now.error;
  return (
    <div className="thread__held" role="group" aria-label="Held to send by itself">
      <p className="small">{describeHeld(entry)}</p>
      <div className="row gap-2">
        <Button size="sm" variant="primary" disabled={busy} onClick={() => now.mutate(entry.id)}>
          Send it now
        </Button>
        <Button size="sm" variant="ghost" disabled={busy} onClick={() => stop.mutate(entry.id)}>
          Stop it
        </Button>
      </div>
      {error ? <InlineError>{error.message}</InlineError> : null}
    </div>
  );
}
