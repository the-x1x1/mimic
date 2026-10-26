import { Button, InlineError } from "@mimic/ui";
import { describeHeld, type OutboxEntry } from "@mimic/contracts";
import { useAutoReply, useHeldReply, useSetAutoReply } from "@/hooks/useSending";

/**
 * A reply Mimic is holding to send by itself, under the message it answers:
 * when it will go and to whom, with the two things the user can do about it
 * — let it go now, or stop it, after which the draft waits for them like any
 * other. Every rule it was held under is checked again when it goes.
 *
 * It can also be stopped for good for its person (0.10.0-alpha.37): they go
 * on the rules' `never` list, and come off the chosen ones, so nothing to
 * them goes by itself again until the user says so in Settings.
 *
 * While rehearsing (0.10.0-alpha.45) it says so, and offers no Send it now:
 * nothing goes, and the reply is the user's once its time comes or it is
 * stopped.
 */
export function HeldReply({ entry }: { entry: OutboxEntry }) {
  const { stop, now } = useHeldReply();
  const auto = useAutoReply();
  const save = useSetAutoReply();
  const rules = auto.data?.rules;
  const rehearsing = Boolean(rules?.rehearse);
  const person = entry.participantId;
  const busy = stop.isPending || now.isPending || save.isPending;
  const error = stop.error ?? now.error ?? save.error;

  async function stopForThem() {
    if (!rules || !person) return;
    await stop.mutateAsync(entry.id);
    await save.mutateAsync({
      ...rules,
      never: [...rules.never.filter((p) => p !== person), person],
      people: rules.people.filter((p) => p !== person),
    });
  }
  return (
    <div className="thread__held" role="group" aria-label="Held to send by itself">
      <p className="small">{describeHeld(entry, new Date(), rehearsing)}</p>
      <div className="row gap-2">
        {rehearsing ? null : (
          <Button size="sm" variant="primary" disabled={busy} onClick={() => now.mutate(entry.id)}>
            Send it now
          </Button>
        )}
        <Button size="sm" variant="ghost" disabled={busy} onClick={() => stop.mutate(entry.id)}>
          Stop it
        </Button>
        {rules && person ? (
          <Button
            size="sm"
            variant="ghost"
            disabled={busy}
            onClick={() => void stopForThem().catch(() => undefined)}
          >
            Stop it, and never by myself to them
          </Button>
        ) : null}
      </div>
      {error ? <InlineError>{error.message}</InlineError> : null}
    </div>
  );
}
