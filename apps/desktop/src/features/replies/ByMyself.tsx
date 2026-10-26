import type { AutoReplyRules } from "@mimic/contracts";
import { useAutoReply, useSetAutoReply } from "@/hooks/useSending";

/**
 * Under a reply, while replying by myself is on: whether replies to this
 * person may go by themselves, and one click to change that for them
 * (0.10.0-alpha.41) — onto or off the rules' `never` list, and onto the
 * chosen people where only those are answered. Says nothing while replying
 * by myself is off, or for a reply to no one in particular.
 */
export function ByMyself({ participantId }: { participantId: string | null }) {
  const state = useAutoReply();
  const save = useSetAutoReply();
  const rules = state.data?.rules;
  if (!rules?.enabled || !participantId) return null;
  const p = participantId;
  const without = (list: string[]) => list.filter((x) => x !== p);
  const change = (next: Partial<AutoReplyRules>) => save.mutate({ ...rules, ...next });

  let line: string;
  let action: string;
  let next: Partial<AutoReplyRules>;
  if (rules.never.includes(p)) {
    line = "I don't answer them by myself: you'd rather answer them yourself.";
    action = "Let me answer them by myself";
    next =
      rules.who === "listed"
        ? { never: without(rules.never), people: [...without(rules.people), p] }
        : { never: without(rules.never) };
  } else if (rules.who === "listed" && !rules.people.includes(p)) {
    line = "I answer only the people you chose by myself, and they aren't one.";
    action = "Answer them by myself too";
    next = { people: [...rules.people, p] };
  } else {
    line =
      rules.who === "listed"
        ? "I may answer them by myself: they're one of the people you chose."
        : "I may answer them by myself, once you've written to them.";
    action = "Never by myself to them";
    next = { never: [...without(rules.never), p], people: without(rules.people) };
  }

  return (
    <p className="muted small" role="group" aria-label="Replying by myself to them">
      {line}{" "}
      <button
        type="button"
        className="linkish"
        disabled={save.isPending}
        onClick={() => change(next)}
      >
        {action}
      </button>
      {save.error ? <span className="danger"> {save.error.message}</span> : null}
    </p>
  );
}
