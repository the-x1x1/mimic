import { useState } from "react";
import { Button, Card, InlineError } from "@mimic/ui";
import { HOLD_CHOICES, describeAutoReply, hourOf, type AutoReplyRules } from "@mimic/contracts";

const HOURS = Array.from({ length: 24 }, (_, h) => h);
import { useAutoReply, useSetAutoReply } from "@/hooks/useSending";
import { useSources } from "@/hooks/useSources";
import { usePeople } from "@/hooks/usePeople";

/**
 * Replying by myself — Trusted mode. Off until turned on here, and turning
 * it on is two steps: the first says exactly what it will do, in the same
 * words as `docs/PRODUCT.md`, and only the second turns it on. Turning it off
 * is one step, and stops every reply still held at once.
 *
 * It can be rehearsed first (0.10.0-alpha.45, `rehearse`): everything as for
 * real, held and shown, and stopped when it would go. Stopping rehearsing
 * leaves what was held while rehearsing to the user.
 */
export function AutoReplySection() {
  const state = useAutoReply();
  const save = useSetAutoReply();
  const sources = useSources();
  const [confirming, setConfirming] = useState(false);
  const [filter, setFilter] = useState("");
  const rules = state.data?.rules ?? null;
  const people = usePeople(false, rules !== null);
  // Mailboxes, Slack workspaces (0.10.0-alpha.66), Teams accounts (.68),
  // Telegram accounts (.76) and Google Chat accounts (.80).
  const isChat = (connector: string) =>
    connector === "slack" ||
    connector === "teams" ||
    connector === "telegram_account" ||
    connector === "google_chat";
  const mailboxes = (sources.data ?? []).filter(
    (s) => s.connector === "imap" || isChat(s.connector),
  );
  const chats = mailboxes.some((s) => isChat(s.connector));

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
        I&rsquo;ve checked your mailbox again, in case you answered from somewhere else; then I
        check everything once more, under your rules as they are then, and look in your Sent folder
        for a reply already there &mdash; from me on your other computer, too. I only answer mail
        that arrives after you turn this on, that I read the same day, and that was sent to you by
        name, not only copied to you &mdash; never mail that looks automated, a mailing list, a
        message with an attachment or an invitation, mail your rules filed in another folder, a
        thread you said needs no reply, a reply you asked me to write, or the same message twice. A
        reply with a gap left to fill in, a link they didn&rsquo;t send, or talk of being an AI
        waits for you. I stop at your daily limit; each reply tells other mail programs it was sent
        automatically, so their automatic replies don&rsquo;t answer it; and nothing I send by
        myself is read as how you write. You can rehearse it first: I do all of this, and send
        nothing.
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
            ones I send. You can turn this off here at any time. Or rehearse first: I hold and show
            each reply as if I&rsquo;d send it, and send none.
          </p>
          <div className="row gap-2">
            <Button
              variant="primary"
              disabled={save.isPending}
              onClick={() => {
                change({ enabled: true, rehearse: false });
                setConfirming(false);
              }}
            >
              Turn it on
            </Button>
            <Button
              disabled={save.isPending}
              onClick={() => {
                change({ enabled: true, rehearse: true });
                setConfirming(false);
              }}
            >
              Rehearse first
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
        <legend className="muted small">
          {chats ? "Through these mailboxes and chats" : "Through these mailboxes"}
        </legend>
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
        {chats ? (
          <p className="muted small">
            In Slack and Teams I answer a one-to-one conversation only, never a group, and just
            before a reply goes I look in the conversation: if you&rsquo;ve answered there, or
            they&rsquo;ve written again, it doesn&rsquo;t go.
          </p>
        ) : null}
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
      <div className="stack gap-1">
        <label className="row gap-2">
          <input
            type="checkbox"
            checked={Boolean(rules.hours)}
            disabled={save.isPending}
            onChange={(e) => change({ hours: e.target.checked ? { from: 8, to: 20 } : null })}
          />
          <span>Only at the hours I choose, on this computer&rsquo;s clock</span>
        </label>
        {rules.hours ? (
          <div className="row gap-2">
            <label className="stack gap-1">
              <span className="muted small">From</span>
              <select
                value={rules.hours.from}
                disabled={save.isPending}
                onChange={(e) => {
                  if (rules.hours)
                    change({ hours: { ...rules.hours, from: Number(e.target.value) } });
                }}
              >
                {HOURS.map((h) => (
                  <option key={h} value={h} disabled={h === rules.hours?.to}>
                    {hourOf(h)}
                  </option>
                ))}
              </select>
            </label>
            <label className="stack gap-1">
              <span className="muted small">Until</span>
              <select
                value={rules.hours.to}
                disabled={save.isPending}
                onChange={(e) => {
                  if (rules.hours)
                    change({ hours: { ...rules.hours, to: Number(e.target.value) } });
                }}
              >
                {HOURS.map((h) => (
                  <option key={h} value={h} disabled={h === rules.hours?.from}>
                    {hourOf(h)}
                  </option>
                ))}
              </select>
            </label>
          </div>
        ) : null}
        {rules.hours ? (
          <p className="muted small">
            A reply that would go outside them waits until they start, on the home screen, where you
            can still stop it or send it at once.
          </p>
        ) : null}
      </div>
      <div className="stack gap-1">
        <label className="row gap-2">
          <input
            type="checkbox"
            checked={Boolean(rules.rehearse)}
            disabled={save.isPending}
            onChange={(e) => change({ rehearse: e.target.checked })}
          />
          <span>Rehearse: hold and show each reply as if I&rsquo;d send it, and send none</span>
        </label>
        {rules.rehearse ? (
          <p className="muted small">
            When you stop rehearsing, the replies I held while rehearsing wait for you; only new
            ones go by themselves.
          </p>
        ) : null}
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
