import { useId, useState } from "react";
import { Button, InlineError } from "@mimic/ui";
import { listAddresses, sendMayHaveGone, type Draft, type ReplyEnvelope } from "@mimic/contracts";
import { useSendReply } from "@/hooks/useSending";
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

  async function confirm() {
    const entry = await send.mutateAsync({
      draftId: draft.id,
      text,
      sourceId,
      to,
      cc,
      subject,
    });
    toast.success(
      entry.cc.length > 0
        ? `Sent to ${listAddresses(entry.to)}, copying in ${listAddresses(entry.cc)}.`
        : `Sent to ${listAddresses(entry.to)}.`,
      text !== draft.generatedText
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
          <select id={`${id}-from`} value={sourceId} onChange={(e) => setSourceId(e.target.value)}>
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
      <div className="thread__send-row">
        <label htmlFor={`${id}-subject`} className="muted small">
          Subject
        </label>
        <input
          id={`${id}-subject`}
          type="text"
          value={subject}
          maxLength={300}
          onChange={(e) => setSubject(e.target.value)}
        />
      </div>
      <p className="muted small">
        It goes from your mailbox{from?.server ? ` (through ${from.server})` : ""}, threaded under
        their message, to {to.length === 1 ? "them" : "those addresses"}
        {cc.length > 0
          ? `, copying in ${listAddresses(cc)} — no one else.`
          : others.length > 0
            ? " only — not to anyone else on the thread unless you copy them in."
            : " only — not to anyone else on the thread."}
      </p>
      {send.error ? (
        <InlineError>
          {unsure ? `${send.error.message} I won't send it again by myself.` : send.error.message}
        </InlineError>
      ) : null}
      <div className="thread__actions">
        <Button
          variant="primary"
          onClick={() =>
            void confirm().catch(() => {
              // Said above, from the mutation's own error.
            })
          }
          disabled={send.isPending || unsure || to.length === 0 || !sourceId || !text.trim()}
        >
          {send.isPending ? "Sending…" : "Send it"}
        </Button>
        <Button variant="ghost" onClick={onCancel} disabled={send.isPending}>
          Not yet
        </Button>
      </div>
    </div>
  );
}
