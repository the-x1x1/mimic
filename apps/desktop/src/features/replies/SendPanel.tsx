import { useEffect, useId, useRef, useState } from "react";
import { Button, InlineError } from "@mimic/ui";
import { listAddresses, sendMayHaveGone, type Draft, type ReplyEnvelope } from "@mimic/contracts";
import { useSendReply } from "@/hooks/useSending";
import { useSettings } from "@/hooks/useSystem";
import { toast } from "@/state/toast";

/**
 * The last look before a reply goes: which mailbox it goes out of, who it
 * goes to, under what subject, and the words — the ones on screen, as the
 * user left them. Nothing is sent until **Send it**, and what is sent is
 * exactly what this shows.
 *
 * Who it goes to starts where the sender asked for replies (their
 * `Reply-To`), or the address they wrote from; any other address Mimic has
 * for them can be chosen instead. A reply goes to the person who wrote, and
 * to the others on their message only when ticked under **Copy in**
 * (`othersOnIt`, none ticked to begin with), and says which.
 *
 * A reply to a message read from Slack (`envelope.kind`, 0.10.0-alpha.65)
 * goes into the conversation it came from, as the user: the workspace is
 * the one "mailbox", who it goes to is that conversation, and there is no
 * subject, signature or anyone to copy in.
 *
 * **Send it** waits the seconds the user chose (`send.undoSeconds`, five
 * unless changed) before anything goes, with **Don't send it** to take it
 * back (0.10.0-alpha.40). Leaving the panel in that time sends nothing, and
 * says so.
 */
export function SendPanel({
  draft,
  text,
  envelope,
  onSent,
  onCancel,
}: {
  /** The way of saying it being sent: the first draft, or one beside it. */
  draft: Draft;
  text: string;
  envelope: ReplyEnvelope;
  onSent: () => void;
  onCancel: () => void;
}) {
  const send = useSendReply();
  const id = useId();
  // Into a chat service: Slack (0.10.0-alpha.65) or Teams (.68).
  const service = envelope.kind === "slack" ? "Slack" : envelope.kind === "teams" ? "Teams" : null;
  const settings = useSettings();
  const wait = settings.data?.["send.undoSeconds"] ?? 5;
  // The countdown before it goes: seconds left, or null when not counting.
  const [left, setLeft] = useState<number | null>(null);
  const counting = useRef<{ timer: number | null }>({ timer: null });
  useEffect(() => {
    const c = counting.current;
    return () => {
      if (c.timer !== null) {
        window.clearInterval(c.timer);
        toast.info("Not sent", "You left before it went, so it didn't.");
      }
    };
  }, []);
  const able = envelope.mailboxes.filter((m) => m.fromAddress !== null);
  const [sourceId, setSourceId] = useState(envelope.mailbox ?? able[0]?.sourceId ?? "");
  // Each choice is a list: where they asked for replies may be several.
  const choices: string[][] = [
    ...(envelope.to.length > 0 ? [envelope.to] : []),
    ...envelope.otherAddresses.map((a) => [a]),
  ];
  const [choice, setChoice] = useState(0);
  const [subject, setSubject] = useState(envelope.subject);
  const others = envelope.othersOnIt;
  const [ticked, setTicked] = useState<ReadonlySet<string>>(new Set());
  // In the order the message had them.
  const cc = others.filter((a) => ticked.has(a));
  const to = choices[choice] ?? [];
  const from = able.find((m) => m.sourceId === sourceId);
  const code = (send.error as { code?: string } | null)?.code;
  const unsure = sendMayHaveGone(code);

  function stopCounting() {
    if (counting.current.timer !== null) window.clearInterval(counting.current.timer);
    counting.current.timer = null;
    setLeft(null);
  }

  /** Send it: after the wait, as it stood when pressed — or at once. */
  function sendIt() {
    const go = () =>
      void confirm().catch(() => {
        // Said below, from the mutation's own error.
      });
    if (wait <= 0) {
      go();
      return;
    }
    let n = wait;
    setLeft(n);
    counting.current.timer = window.setInterval(() => {
      n -= 1;
      if (n > 0) {
        setLeft(n);
        return;
      }
      stopCounting();
      go();
    }, 1000);
  }

  async function confirm() {
    const entry = await send.mutateAsync({
      draftId: draft.id,
      text,
      sourceId,
      to,
      cc,
      subject,
    });
    const changed = text !== draft.generatedText;
    if (service) {
      toast.success(
        `Sent to ${listAddresses(entry.to)} in ${service}.`,
        changed
          ? "It went in as you, and I noticed what you changed."
          : "It went in as you, as if you'd written it there yourself.",
      );
      onSent();
      return;
    }
    toast.success(
      entry.cc.length > 0
        ? `Sent to ${listAddresses(entry.to)}, copying in ${listAddresses(entry.cc)}.`
        : `Sent to ${listAddresses(entry.to)}.`,
      changed
        ? "It went from your mailbox, and I noticed what you changed."
        : "It went from your mailbox, as if you'd sent it yourself.",
    );
    onSent();
  }

  return (
    <div className="thread__send" role="group" aria-label="Send it">
      <div className="thread__send-row">
        <label htmlFor={`${id}-from`} className="muted small">
          From
        </label>
        {able.length > 1 ? (
          <select
            id={`${id}-from`}
            value={sourceId}
            disabled={left !== null}
            onChange={(e) => setSourceId(e.target.value)}
          >
            {able.map((m) => (
              <option key={m.sourceId} value={m.sourceId}>
                {m.fromAddress}
              </option>
            ))}
          </select>
        ) : (
          <span id={`${id}-from`}>{from?.fromAddress}</span>
        )}
      </div>
      <div className="thread__send-row">
        <label htmlFor={`${id}-to`} className="muted small">
          To
        </label>
        {choices.length > 1 ? (
          <select
            id={`${id}-to`}
            value={choice}
            disabled={left !== null}
            onChange={(e) => setChoice(Number(e.target.value))}
          >
            {choices.map((c, i) => (
              <option key={c.join(",")} value={i}>
                {listAddresses(c)}
              </option>
            ))}
          </select>
        ) : (
          <span id={`${id}-to`}>{listAddresses(to)}</span>
        )}
      </div>
      {others.length > 0 ? (
        <div
          className="thread__send-row thread__send-row--top"
          role="group"
          aria-labelledby={`${id}-cc`}
        >
          <span id={`${id}-cc`} className="muted small">
            Copy in
          </span>
          <div className="stack gap-1">
            {others.length > 1 ? (
              <label className="row gap-1">
                <input
                  type="checkbox"
                  checked={cc.length === others.length}
                  disabled={left !== null}
                  onChange={(e) => setTicked(new Set(e.target.checked ? others : []))}
                />
                <span>Everyone else on their message</span>
              </label>
            ) : null}
            {others.map((a) => (
              <label key={a} className="row gap-1">
                <input
                  type="checkbox"
                  checked={ticked.has(a)}
                  disabled={left !== null}
                  onChange={(e) => {
                    const next = new Set(ticked);
                    if (e.target.checked) next.add(a);
                    else next.delete(a);
                    setTicked(next);
                  }}
                />
                <span>{a}</span>
              </label>
            ))}
          </div>
        </div>
      ) : null}
      {from?.signature ? (
        <div className="thread__send-row thread__send-row--top">
          <span className="muted small">Signed</span>
          <p className="letter signature-preview">{from.signature}</p>
        </div>
      ) : null}
      {service ? null : (
        <div className="thread__send-row">
          <label htmlFor={`${id}-subject`} className="muted small">
            Subject
          </label>
          <input
            id={`${id}-subject`}
            type="text"
            value={subject}
            maxLength={300}
            disabled={left !== null}
            onChange={(e) => setSubject(e.target.value)}
          />
        </div>
      )}
      {service ? (
        <p className="muted small">
          It goes into that conversation in {service}, as you, and nowhere else &mdash; word for
          word, with nothing in it read as a mention.
        </p>
      ) : (
        <p className="muted small">
          It goes from your mailbox{from?.server ? ` (through ${from.server})` : ""}, threaded under
          their message, to {to.length === 1 ? "them" : "those addresses"}
          {cc.length > 0
            ? `, copying in ${listAddresses(cc)} — no one else.`
            : others.length > 0
              ? " only — not to anyone else on the thread unless you copy them in."
              : " only — not to anyone else on the thread."}
        </p>
      )}
      {send.error ? (
        <InlineError>
          {unsure ? `${send.error.message} I won't send it again by myself.` : send.error.message}
        </InlineError>
      ) : null}
      {left !== null ? (
        <div className="thread__actions" role="status">
          <span>
            Sending in {left} second{left === 1 ? "" : "s"}&hellip;
          </span>
          <Button variant="primary" onClick={stopCounting}>
            Don&rsquo;t send it
          </Button>
        </div>
      ) : (
        <div className="thread__actions">
          <Button
            variant="primary"
            onClick={sendIt}
            // Not before the wait the user chose is known: a press then
            // would count down five seconds where they chose none.
            disabled={
              send.isPending ||
              settings.isPending ||
              unsure ||
              to.length === 0 ||
              !sourceId ||
              !text.trim()
            }
          >
            {send.isPending ? "Sending…" : "Send it"}
          </Button>
          <Button variant="ghost" onClick={onCancel} disabled={send.isPending}>
            Not yet
          </Button>
        </div>
      )}
    </div>
  );
}
