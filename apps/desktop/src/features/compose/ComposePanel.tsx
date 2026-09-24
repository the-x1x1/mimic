import { useMemo, useState } from "react";
import { Button, Card, Field, InlineError } from "@mimic/ui";
import {
  ADJUSTMENT_LABELS,
  Adjustment,
  adjustmentOf,
  CHANNEL_LABELS,
  Channel,
  composeReadiness,
  type ComposeRequest,
  type Draft,
} from "@mimic/contracts";
import { EvidencePanel } from "@/components/EvidencePanel";
import { usePeople } from "@/hooks/usePeople";
import {
  useGenerateDraft,
  useGenerationContext,
  useProviderHealth,
  useProviderState,
  useResolveDraft,
} from "@/hooks/useCompose";
import { toast } from "@/state/toast";

/**
 * Writing something new: four inputs, one output, and a panel that says what
 * the output was based on.
 *
 * This lives on the dashboard rather than on a screen of its own, because the
 * two halves of the product are the same job — replying to what is waiting,
 * and starting something. The intent field stays the centre of it: Mimic
 * supplies how the user writes, the user supplies what they want to say, so it
 * is the largest input here and the one focused first.
 */
export function ComposePanel({ title = "Write something new" }: { title?: string }) {
  const people = usePeople();
  const providers = useProviderState();
  const generate = useGenerateDraft();
  const resolve = useResolveDraft();

  const [participantId, setParticipantId] = useState<string>("");
  const [channel, setChannel] = useState<Channel>("email");
  const [incoming, setIncoming] = useState("");
  const [intent, setIntent] = useState("");
  const [draft, setDraft] = useState<Draft | null>(null);
  const [edited, setEdited] = useState<string>("");

  const request: ComposeRequest = useMemo(
    () => ({
      participantId: participantId || null,
      channel,
      incomingMessage: incoming.trim() || null,
      intent: intent.trim() || null,
    }),
    [participantId, channel, incoming, intent],
  );

  const context = useGenerationContext(request, true);
  const activeProvider = providers.data?.providers.find((p) => p.id === providers.data?.active);
  const health = useProviderHealth(activeProvider?.id);
  const readiness = composeReadiness(
    context.data,
    activeProvider
      ? {
          displayName: activeProvider.displayName,
          local: activeProvider.local,
          reachable: health.data ? health.data.reachable : null,
          error: health.data?.error ?? null,
        }
      : undefined,
  );
  const dirty = draft !== null && edited !== draft.generatedText;
  // A draft asked for shorter, longer or in another register is not read for
  // habits: what is changed in it was changed from that request.
  const adjusted = draft ? adjustmentOf(draft) : null;

  async function run(adjustment?: Adjustment) {
    const result = await generate.mutateAsync({ ...request, adjustment: adjustment ?? null });
    // The draft it replaces was passed over for this one, not left pending
    // for ever: recorded as regenerated, which is neither sent nor turned down.
    if (draft && draft.outcome === null) {
      await resolve
        .mutateAsync({ draftId: draft.id, outcome: "regenerated", finalText: null })
        .catch(() => undefined);
    }
    setDraft(result);
    setEdited(result.generatedText);
  }

  async function markSent() {
    if (!draft) return;
    await resolve.mutateAsync({
      draftId: draft.id,
      outcome: dirty ? "sent_edited" : "sent_unedited",
      finalText: edited,
    });
    toast.success(
      dirty && !adjusted ? "Saved, and Mimic noted what you changed" : "Saved",
      adjusted
        ? `This one was written ${ADJUSTMENT_LABELS[adjusted].toLowerCase()}, so Mimic doesn't learn your habits from it.`
        : dirty
          ? undefined
          : "Nothing to learn from this one — you sent it as written.",
    );
    setDraft(null);
    setEdited("");
    setIntent("");
  }

  async function copy() {
    await navigator.clipboard.writeText(edited);
    toast.info("Copied");
  }

  return (
    <div className="compose">
      <div className="compose__header">
        <h2>{title}</h2>
        <p className="muted small">
          {activeProvider ? (
            <>
              Drafting with <strong>{activeProvider.displayName}</strong>
              {activeProvider.local
                ? " — nothing leaves this computer."
                : " — your message is sent to this provider."}
            </>
          ) : (
            "No model provider configured yet."
          )}
        </p>
      </div>

      <div className="compose__grid">
        <div className="stack gap-3">
          <Card title="Who and where">
            <div className="row gap-2">
              <Field label="Recipient" htmlFor="recipient">
                <select
                  id="recipient"
                  aria-describedby={
                    people.data && people.data.people.length < people.data.peopleTotal
                      ? "recipient-note"
                      : undefined
                  }
                  value={participantId}
                  onChange={(e) => setParticipantId(e.target.value)}
                >
                  <option value="">Someone not on this list</option>
                  {(people.data?.people ?? []).map((p) => (
                    <option key={p.participant.id} value={p.participant.id}>
                      {p.participant.displayName}
                      {p.participant.relationship ? ` · ${p.participant.relationship}` : ""}
                    </option>
                  ))}
                </select>
                {people.data && people.data.people.length < people.data.peopleTotal ? (
                  <span id="recipient-note" className="muted small">
                    The {people.data.people.length} people you&rsquo;ve been in touch with most
                    recently, of {people.data.peopleTotal.toLocaleString()}.
                  </span>
                ) : null}
              </Field>
              <Field label="Channel" htmlFor="channel">
                <select
                  id="channel"
                  value={channel}
                  onChange={(e) => setChannel(e.target.value as Channel)}
                >
                  {Channel.options.map((c) => (
                    <option key={c} value={c}>
                      {CHANNEL_LABELS[c]}
                    </option>
                  ))}
                </select>
              </Field>
            </div>
          </Card>

          <Card title="What you are replying to">
            <textarea
              rows={5}
              value={incoming}
              placeholder="Paste the message you received. Leave this empty if you are starting the conversation."
              onChange={(e) => setIncoming(e.target.value)}
            />
          </Card>

          <Card title="What you want to say">
            <textarea
              className="compose__intent"
              rows={4}
              autoFocus
              value={intent}
              placeholder="In your own shorthand: yes but push to Thursday; decline, no reason; thank them and ask about the invoice."
              onChange={(e) => setIntent(e.target.value)}
            />
            <p className="muted small">
              Mimic decides how it is said. You decide what is said — it will not commit you to
              anything you have not written here.
            </p>
            <div className="row gap-2">
              <Button
                variant="primary"
                onClick={() => run()}
                disabled={generate.isPending || !readiness.ready}
                title={readiness.ready ? undefined : (readiness.reason ?? undefined)}
              >
                {generate.isPending ? "Writing…" : draft ? "Regenerate" : "Write a draft"}
              </Button>
              {draft
                ? Adjustment.options.map((a) => (
                    <Button
                      key={a}
                      variant="ghost"
                      onClick={() => run(a)}
                      disabled={generate.isPending || !readiness.ready}
                    >
                      {ADJUSTMENT_LABELS[a]}
                    </Button>
                  ))
                : null}
            </div>
            {generate.isError ? (
              <InlineError>{(generate.error as Error).message}</InlineError>
            ) : null}
          </Card>

          {draft ? (
            <Card title="Draft">
              <textarea
                className="compose__draft"
                rows={8}
                value={edited}
                onChange={(e) => setEdited(e.target.value)}
              />
              <div className="row gap-2">
                <Button variant="primary" onClick={markSent}>
                  {dirty ? "I sent my edited version" : "I sent this"}
                </Button>
                <Button variant="ghost" onClick={copy}>
                  Copy
                </Button>
                <Button
                  variant="ghost"
                  onClick={async () => {
                    await resolve.mutateAsync({
                      draftId: draft.id,
                      outcome: "discarded",
                      finalText: null,
                    });
                    setDraft(null);
                  }}
                >
                  Discard
                </Button>
              </div>
              <p className="muted small">
                {adjusted
                  ? `This draft was written ${ADJUSTMENT_LABELS[adjusted].toLowerCase()}, so Mimic won't learn your habits from what you change in it. Nothing is sent for you.`
                  : dirty
                    ? "Mimic will compare what it wrote with what you send, and use the difference."
                    : "Telling Mimic what you actually sent is the only way it improves. Nothing is sent for you."}
              </p>
            </Card>
          ) : null}
        </div>

        <div className="stack gap-3">
          {readiness.reason ? (
            <Card title={readiness.ready ? "Worth knowing" : "Mimic cannot draft right now"}>
              {readiness.reason}
            </Card>
          ) : null}
          <EvidencePanel context={context.data} loading={context.isLoading} />
        </div>
      </div>
    </div>
  );
}
