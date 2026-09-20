import { useMemo, useState } from "react";
import { Link } from "react-router-dom";
import { Button, Card, EmptyState, Field, InlineError } from "@mimic/ui";
import {
  ADJUSTMENT_LABELS,
  Adjustment,
  CHANNEL_LABELS,
  Channel,
  composeReadiness,
  type ComposeRequest,
  type Draft,
} from "@mimic/contracts";
import { PageHeader } from "@/components/PageHeader";
import { EvidencePanel } from "@/components/EvidencePanel";
import { usePeople } from "@/hooks/usePeople";
import {
  useGenerateDraft,
  useGenerationContext,
  useProviderState,
  useResolveDraft,
} from "@/hooks/useCompose";
import { toast } from "@/state/toast";

/**
 * The Compose screen. Four inputs, one output, and a panel that says what the
 * output was based on.
 *
 * The intent field is the centre of the product, not an afterthought: Mimic
 * supplies how the user writes, the user supplies what they want to say. It is
 * therefore the largest input on the screen and the one focused first.
 */
export function ComposePage() {
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
  const readiness = composeReadiness(context.data);
  const activeProvider = providers.data?.providers.find((p) => p.id === providers.data?.active);
  const dirty = draft !== null && edited !== draft.generatedText;

  async function run(adjustment?: Adjustment) {
    const result = await generate.mutateAsync({ ...request, adjustment: adjustment ?? null });
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
      dirty ? "Saved, and Mimic noted what you changed" : "Saved",
      dirty ? undefined : "Nothing to learn from this one — you sent it as written.",
    );
    setDraft(null);
    setEdited("");
    setIntent("");
  }

  async function copy() {
    await navigator.clipboard.writeText(edited);
    toast.info("Copied");
  }

  if (people.isSuccess && people.data.length === 0 && !context.data?.effective.measurable) {
    return (
      <EmptyState
        title="Nothing to work from yet"
        body="Mimic writes in your voice by reading messages you have already written. Add a source and import some, and this screen becomes useful."
        primary={
          <Link to="/sources">
            <Button variant="primary">Add a source</Button>
          </Link>
        }
      />
    );
  }

  return (
    <div className="compose">
      <PageHeader
        title="Compose"
        subtitle={
          activeProvider ? (
            <>
              Drafting with <strong>{activeProvider.displayName}</strong>
              {activeProvider.local
                ? " — nothing leaves this computer."
                : " — your message is sent to this provider."}
            </>
          ) : (
            "No model provider configured yet."
          )
        }
      />

      <div className="compose__grid">
        <div className="stack gap-3">
          <Card title="Who and where">
            <div className="row gap-2">
              <Field label="Recipient" htmlFor="recipient">
                <select
                  id="recipient"
                  value={participantId}
                  onChange={(e) => setParticipantId(e.target.value)}
                >
                  <option value="">Someone Mimic has not seen</option>
                  {(people.data ?? []).map((p) => (
                    <option key={p.participant.id} value={p.participant.id}>
                      {p.participant.displayName}
                      {p.participant.relationship ? ` · ${p.participant.relationship}` : ""}
                    </option>
                  ))}
                </select>
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
              <Button variant="primary" onClick={() => run()} disabled={generate.isPending}>
                {generate.isPending ? "Writing…" : draft ? "Regenerate" : "Write a draft"}
              </Button>
              {draft
                ? Adjustment.options.map((a) => (
                    <Button
                      key={a}
                      variant="ghost"
                      onClick={() => run(a)}
                      disabled={generate.isPending}
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
                {dirty
                  ? "Mimic will compare what it wrote with what you send, and use the difference."
                  : "Telling Mimic what you actually sent is the only way it improves. Nothing is sent for you."}
              </p>
            </Card>
          ) : null}
        </div>

        <div className="stack gap-3">
          {readiness.reason ? <Card title="Worth knowing">{readiness.reason}</Card> : null}
          <EvidencePanel context={context.data} loading={context.isLoading} />
        </div>
      </div>
    </div>
  );
}
