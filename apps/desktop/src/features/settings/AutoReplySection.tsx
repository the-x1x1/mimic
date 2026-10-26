import { useState } from "react";
import { Button, Card, InlineError } from "@mimic/ui";
import { HOLD_CHOICES, describeAutoReply, type AutoReplyRules } from "@mimic/contracts";
import { useAutoReply, useSetAutoReply } from "@/hooks/useSending";
import { useSources } from "@/hooks/useSources";
import { usePeople } from "@/hooks/usePeople";

/**
 * Replying by myself — Trusted mode. Off until turned on here, and turning
 * it on is two steps: the first says exactly what it will do, in the same
 * words as `docs/PRODUCT.md`, and only the second turns it on. Turning it off
 * is one step, and stops every reply still held at once.
 */
export function AutoReplySection() {
  const state = useAutoReply();
  const save = useSetAutoReply();
  const sources = useSources();
  const [confirming, setConfirming] = useState(false);
  const [filter, setFilter] = useState("");
  const rules = state.data?.rules ?? null;
  const people = usePeople(false, rules !== null);
  const mailboxes = (sources.data ?? []).filter((s) => s.connector === "imap");

  if (!rules || !state.data) {
    return (
      <Card title="Replying by myself">
        {state.error ? (
          <InlineError>{state.error.message}</InlineError>
        ) : (
          <p className="muted">…</p>
        )}
      </Card>
    );
  }

  const change = (next: Partial<AutoReplyRules>) => save.mutate({ ...rules, ...next });
  const toggle = (list: string[], id: string, on: boolean) =>
    on ? [...list.filter((x) => x !== id), id] : list.filter((x) => x !== id);
  const shown = (people.data?.people ?? []).filter((p) =>
    p.participant.displayName.toLowerCase().includes(filter.trim().toLowerCase()),
  );
  // Left out by name: those named, until a search (with anyone it may
  // answer) says whom to look for. Someone named but not among the people
  // loaded here is still listed, to be taken off.
  const names = new Map(
    (people.data?.people ?? []).map((p) => [p.participant.id, p.participant.displayName]),
  );
  const leftOut =
    filter.trim() && rules.who === "known"
      ? shown.map((p) => ({ id: p.participant.id, name: p.participant.displayName }))
      : rules.never.map((id) => ({ id, name: names.get(id) ?? "Someone else you named" }));

  return (
    <Card title="Replying by myself" id="by-myself">
      <p>{describeAutoReply(state.data)}</p>
      <p className="neutral">
        With this on, I send the replies I prepare in advance by myself &mdash; from your mailbox,
        as you, to the address they wrote from. Each one waits on the home screen first, under the
        message it answers, where you can stop it or send it at once, and it doesn&rsquo;t go until
        I&rsquo;ve checked your mailbox again, in case you answered from somewhere else. I only
        answer mail that arrives after you turn this on and was sent to you by name &mdash; never
        mail that looks automated, a list, a thread you said needs no reply, a reply you asked me to
        write, or the same message twice; I stop at your daily limit; and each reply tells other
        mail programs it was sent automatically, so their automatic replies don&rsquo;t answer it.
        Nothing I send by myself is read as how you write.
      </p>

      {rules.enabled ? (
        <div className="row gap-2">
          <Button
            variant="danger"
            disabled={save.isPending}
            onClick={() => change({ enabled: false })}
          >
            Turn it off and stop anything waiting
          </Button>
        </div>
      ) : confirming ? (
        <div className="stack gap-2" role="group" aria-label="Turn on replying by myself">
          <p>
            I&rsquo;ll send replies as you without asking each time, to the people and through the
            mailboxes below. Replies prepared in advance will be turned on too, since those are the
            ones I send. You can turn this off here at any time.
          </p>
          <div className="row gap-2">
            <Button
              variant="primary"
              disabled={save.isPending}
              onClick={() => {
                change({ enabled: true });
                setConfirming(false);
              }}
            >
              Turn it on
            </Button>
            <Button variant="ghost" onClick={() => setConfirming(false)}>
              Not now
            </Button>
          </div>
        </div>
      ) : (
        <div className="row gap-2">
          <Button onClick={() => setConfirming(true)}>Reply by myself…</Button>
        </div>
      )}

      <fieldset className="stack gap-1">
        <legend className="muted small">Through these mailboxes</legend>
        {mailboxes.length === 0 ? (
          <p className="muted small">Connect a mailbox under Your mail first.</p>
        ) : (
          mailboxes.map((m) => (
            <label key={m.id} className="row gap-2">
              <input
                type="checkbox"
                checked={rules.mailboxes.includes(m.id)}
                disabled={save.isPending}
                onChange={(e) =>
                  change({ mailboxes: toggle(rules.mailboxes, m.id, e.target.checked) })
                }
              />
              <span>{m.name}</span>
            </label>
          ))
        )}
      </fieldset>

      <fieldset className="stack gap-1">
        <legend className="muted small">To</legend>
        <label className="row gap-2">
          <input
            type="radio"
            name="autoreply-who"
            checked={rules.who === "known"}
            disabled={save.isPending}
            onChange={() => change({ who: "known" })}
          />
          <span>People you&rsquo;ve written to yourself</span>
        </label>
        <label className="row gap-2">
          <input
            type="radio"
            name="autoreply-who"
            checked={rules.who === "listed"}
            disabled={save.isPending}
            onChange={() => change({ who: "listed" })}
          />
          <span>Only the people I choose</span>
        </label>
        {rules.who === "known" || rules.never.length > 0 ? (
          <div className="stack gap-1" role="group" aria-label="Never by myself to">
            <span className="muted small">
              Never by myself to these, whom you&rsquo;d rather answer yourself:
            </span>
            {rules.who === "known" ? (
              <input
                type="search"
                aria-label="Find someone to leave out"
                placeholder="Find someone"
                value={filter}
                onChange={(e) => setFilter(e.target.value)}
              />
            ) : null}
            {leftOut.length === 0 && !filter.trim() ? (
              <p className="muted small">No one yet.</p>
            ) : null}
            {leftOut.slice(0, 50).map((p) => (
              <label key={p.id} className="row gap-2">
                <input
                  type="checkbox"
                  checked={rules.never.includes(p.id)}
                  disabled={save.isPending}
                  onChange={(e) => change({ never: toggle(rules.never, p.id, e.target.checked) })}
                />
                <span>{p.name}</span>
              </label>
            ))}
          </div>
        ) : null}
        {rules.who === "listed" ? (
          <div className="stack gap-1">
            <input
              type="search"
              aria-label="Find someone"
              placeholder="Find someone"
              value={filter}
              onChange={(e) => setFilter(e.target.value)}
            />
            {shown.slice(0, 50).map((p) => (
              <label key={p.participant.id} className="row gap-2">
                <input
                  type="checkbox"
                  checked={
                    rules.people.includes(p.participant.id) &&
                    !rules.never.includes(p.participant.id)
                  }
                  disabled={save.isPending}
                  onChange={(e) =>
                    // Chosen again: off the never list too, or it would
                    // stay ticked here and never be answered.
                    change({
                      people: toggle(rules.people, p.participant.id, e.target.checked),
                      never: e.target.checked
                        ? rules.never.filter((x) => x !== p.participant.id)
                        : rules.never,
                    })
                  }
                />
                <span>{p.participant.displayName}</span>
              </label>
            ))}
          </div>
        ) : null}
      </fieldset>

      <div className="row gap-3">
        <label className="stack gap-1">
          <span className="muted small">Hold each reply for</span>
          <select
            value={rules.holdMinutes}
            disabled={save.isPending}
            onChange={(e) => change({ holdMinutes: Number(e.target.value) })}
          >
            {HOLD_CHOICES.map((m) => (
              <option key={m} value={m}>
                {m} minutes
              </option>
            ))}
          </select>
        </label>
        <label className="stack gap-1">
          <span className="muted small">At most a day</span>
          <input
            type="number"
            min={0}
            max={50}
            value={rules.dailyLimit}
            disabled={save.isPending}
            onChange={(e) => change({ dailyLimit: Number(e.target.value) })}
          />
        </label>
      </div>
      <p className="muted small">
        {state.data.lastDay === 1
          ? "1 reply sent or held by myself in the last day."
          : `${state.data.lastDay.toLocaleString()} replies sent or held by myself in the last day.`}
      </p>
      {save.error ? <InlineError>{save.error.message}</InlineError> : null}
    </Card>
  );
}
