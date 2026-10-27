import { useEffect, useMemo, useRef, useState } from "react";
import { useLocation } from "react-router-dom";
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
import { useSettings } from "@/hooks/useSystem";
import { ipc } from "@/lib/ipc";
import { useAnywhere } from "@/state/anywhere";
import { channelFromTitle, personFromTitle } from "./fromTitle";
import { toast } from "@/state/toast";

/** What a channel read from a window's title is said to be. */
const GUESSED_AS: Record<Channel, string> = {
  chat: "a chat",
  sms: "a text message",
  email: "an email",
  forum: "a forum",
  other: "a message",
};

/**
 * Writing something new: four inputs, one output, and a panel that says what
 * the output was based on.
 *
 * This lives on the dashboard rather than on a screen of its own, because the
 * two halves of the product are the same job — replying to what is waiting,
 * and starting something. The intent field stays the centre of it: Mimic
 * supplies how the user writes, the user supplies what they want to say, so it
 * is the largest input here and the one focused first.
 *
 * Opened from anywhere (the shortcut), a draft can be copied and Mimic steps
 * aside, so the reply can be pasted where the message was. Mimic can't see
 * it go from there, so the next time Write is open it asks whether it did,
 * and records only what the user says (`useAnywhere`).
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
  const [fromAnywhere, setFromAnywhere] = useState(false);
  // Whom the window's title named, when it was chosen for the user.
  const [guessed, setGuessed] = useState<string | null>(null);
  const copiedAway = useAnywhere((s) => s.copied);
  const setCopiedAway = useAnywhere((s) => s.setCopied);
  // Paste it there and send (0.10.0-alpha.58): only once the user turned on
  // pressing Enter after pasting.
  const settings = useSettings();
  const pressEnter = settings.data?.["anywhere.pressEnter"] === true;
  // Straight to a draft (0.10.0-alpha.63): opened from anywhere, one is
  // written as soon as it can be, unless the user turned that off.
  const settingsLoaded = settings.data !== undefined;
  const draftAtOnce = settings.data?.["anywhere.draftAtOnce"] !== false;

  // Opened from anywhere (the shortcut, `anywhere.rs`): what was copied is
  // the message being answered, and a draft for something else goes —
  // recorded as regenerated, as one replaced here is, not left pending.
  const location = useLocation();
  const pasted = (location.state as { pasted?: unknown } | null)?.pasted;
  const current = useRef<Draft | null>(null);
  useEffect(() => {
    current.current = draft;
  }, [draft]);
  const handled = useRef<string | null>(null);
  const lastGuess = useRef<string | null>(null);
  const windowTitle = (location.state as { windowTitle?: unknown } | null)?.windowTitle;
  // What kind of conversation the window's title says it is, while that is
  // still the channel chosen (0.10.0-alpha.83).
  const [channelGuessed, setChannelGuessed] = useState<Channel | null>(null);
  const lastChannelGuess = useRef<Channel | null>(null);
  const resolveAsync = resolve.mutateAsync;
  useEffect(() => {
    if (typeof pasted !== "string" || handled.current === location.key) return;
    // Each time it is opened this way, even on the same words; once each.
    handled.current = location.key;
    const replaced = current.current;
    if (replaced && replaced.outcome === null) {
      void resolveAsync({ draftId: replaced.id, outcome: "regenerated", finalText: null }).catch(
        () => undefined,
      );
    }
    setIncoming(pasted);
    setDraft(null);
    setEdited("");
    setFromAnywhere(true);
    // Whom the last window named was for the last message, not this one.
    const before = lastGuess.current;
    if (before) setParticipantId((id) => (id === before ? "" : id));
    lastGuess.current = null;
    setGuessed(null);
    // A chat answered as a chat, a text as a text, by the window's title;
    // what the last window said goes with its message.
    const kind = typeof windowTitle === "string" ? channelFromTitle(windowTitle) : null;
    const was = lastChannelGuess.current;
    if (kind) setChannel(kind);
    else if (was) setChannel((c) => (c === was ? "email" : c));
    lastChannelGuess.current = kind;
    setChannelGuessed(kind);
  }, [location.key, pasted, windowTitle, resolveAsync]);

  // Who it's from, when the title of the window it was copied in names one
  // person on the list (0.10.0-alpha.52): chosen for them once the list is
  // in, and said while it stays chosen.
  const guessedFor = useRef<string | null>(null);
  // The navigation whose window title has been looked at, so a draft written
  // at once is written to whoever it names.
  const [guessDone, setGuessDone] = useState<string | null>(null);
  const listed = people.data?.people;
  useEffect(() => {
    if (typeof pasted !== "string" || typeof windowTitle !== "string" || !listed) return;
    if (guessedFor.current === location.key) return;
    guessedFor.current = location.key;
    setGuessDone(location.key);
    const found = personFromTitle(
      windowTitle,
      listed.map((p) => ({ id: p.participant.id, name: p.participant.displayName })),
    );
    if (found) {
      setParticipantId(found);
      setGuessed(found);
      lastGuess.current = found;
    }
  }, [location.key, pasted, windowTitle, listed]);
  const guessedName =
    guessed && guessed === participantId
      ? listed?.find((p) => p.participant.id === guessed)?.participant.displayName
      : undefined;

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

  // Which message is on screen now: a draft finished after another was
  // copied in belongs to the one before (0.10.0-alpha.87).
  const showing = useRef(location.key);
  useEffect(() => {
    showing.current = location.key;
  }, [location.key]);

  async function run(adjustment?: Adjustment) {
    const writtenFor = showing.current;
    const result = await generate.mutateAsync({ ...request, adjustment: adjustment ?? null });
    if (showing.current !== writtenFor) {
      // Its message is gone from here: not shown under the next one, and
      // not left waiting either.
      await resolve
        .mutateAsync({ draftId: result.id, outcome: "regenerated", finalText: null })
        .catch(() => undefined);
      return;
    }
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

  // The latest `run`, for the draft written at once: an effect that named
  // `run` would run again on every render.
  const runLatest = useRef(run);
  useEffect(() => {
    runLatest.current = run;
  });
  const autoFor = useRef<string | null>(null);
  useEffect(() => {
    if (typeof pasted !== "string" || !settingsLoaded || !draftAtOnce) return;
    if (autoFor.current === location.key) return;
    // After the message is in, and the window's title has been read.
    if (incoming !== pasted || draft !== null) return;
    if (typeof windowTitle === "string" && guessDone !== location.key) return;
    if (!readiness.ready || generate.isPending) return;
    autoFor.current = location.key;
    void runLatest.current().catch(() => undefined);
  }, [
    pasted,
    settingsLoaded,
    draftAtOnce,
    location.key,
    incoming,
    draft,
    windowTitle,
    guessDone,
    readiness.ready,
    generate.isPending,
  ]);

  // A draft for a message from anywhere puts the keyboard on putting it
  // back: Enter pastes it (and sends it, when that is on).
  const pasteButton = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    if (draft && fromAnywhere) pasteButton.current?.focus();
  }, [draft, fromAnywhere]);

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

  /**
   * Put the reply where the message came from (0.10.0-alpha.46): pasted
   * into the window the shortcut was pressed in, for the user to send there —
   * or, with `send`, Enter pressed after it, which sends in most chat apps
   * (0.10.0-alpha.58); copied, saying why, when it can't be. Then the same
   * question as a copy: whether it went is the user's to say.
   */
  async function pasteThere(send = false) {
    if (!draft) return;
    const text = edited;
    setCopiedAway({ draftId: draft.id, text, generatedText: draft.generatedText });
    setDraft(null);
    setEdited("");
    setIntent("");
    setIncoming("");
    setFromAnywhere(false);
    try {
      const why = send ? await ipc.pasteBack(text, true) : await ipc.pasteBack(text);
      if (why) toast.info(send ? "Not sent" : "Copied", why);
    } catch (e) {
      await navigator.clipboard.writeText(text).catch(() => undefined);
      toast.info(
        "Copied",
        send && e instanceof Error ? e.message : "Paste it where the message was.",
      );
    }
  }

  /** Copy the reply and step aside, to paste it where the message came from. */
  async function copyAndGoBack() {
    if (!draft) return;
    await navigator.clipboard.writeText(edited);
    setCopiedAway({ draftId: draft.id, text: edited, generatedText: draft.generatedText });
    setDraft(null);
    setEdited("");
    setIntent("");
    setIncoming("");
    setFromAnywhere(false);
    try {
      await ipc.stepAside();
    } catch {
      toast.info("Copied", "Paste it where the message was.");
    }
  }

  /** What the user says became of the reply they copied away. */
  async function settleCopied(went: boolean) {
    if (!copiedAway) return;
    const changed = copiedAway.text !== copiedAway.generatedText;
    await resolve.mutateAsync({
      draftId: copiedAway.draftId,
      outcome: went ? (changed ? "sent_edited" : "sent_unedited") : "discarded",
      finalText: went ? copiedAway.text : null,
    });
    setCopiedAway(null);
    toast.success(
      "Noted",
      went
        ? changed
          ? "I'll learn from what you changed before copying it."
          : "You sent it as I wrote it."
        : undefined,
    );
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
          {copiedAway ? (
            <Card title="The reply you copied">
              <blockquote className="letter">
                {copiedAway.text.length > 280
                  ? `${copiedAway.text.slice(0, 280)}…`
                  : copiedAway.text}
              </blockquote>
              <p className="muted small">
                I can&rsquo;t see what became of it after you pasted it. Did it go?
              </p>
              <div className="row gap-2">
                <Button
                  variant="primary"
                  disabled={resolve.isPending}
                  onClick={() => void settleCopied(true).catch(() => undefined)}
                >
                  It went as I copied it
                </Button>
                <Button
                  variant="ghost"
                  disabled={resolve.isPending}
                  onClick={() => setCopiedAway(null)}
                  title="I won't learn from it, since I don't know what you sent."
                >
                  I changed it first
                </Button>
                <Button
                  variant="ghost"
                  disabled={resolve.isPending}
                  onClick={() => void settleCopied(false).catch(() => undefined)}
                >
                  It didn&rsquo;t go
                </Button>
              </div>
              {resolve.isError && copiedAway ? (
                <InlineError>{(resolve.error as Error).message}</InlineError>
              ) : null}
            </Card>
          ) : null}
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
                {guessedName ? (
                  <span className="muted small">
                    From the window you copied it in, this looks like {guessedName}.
                  </span>
                ) : null}
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
                {channelGuessed && channelGuessed === channel ? (
                  <span className="muted small">
                    {`From the window you copied it in, this looks like ${GUESSED_AS[channelGuessed]}.`}
                  </span>
                ) : null}
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
                {fromAnywhere && pressEnter ? (
                  <Button ref={pasteButton} variant="primary" onClick={() => void pasteThere(true)}>
                    Paste it there and send
                  </Button>
                ) : null}
                {fromAnywhere ? (
                  <>
                    <Button
                      ref={pressEnter ? undefined : pasteButton}
                      variant={pressEnter ? "ghost" : "primary"}
                      onClick={() => void pasteThere()}
                    >
                      Paste it there
                    </Button>
                    <Button variant="ghost" onClick={() => void copyAndGoBack()}>
                      Copy and go back
                    </Button>
                  </>
                ) : null}
                <Button variant={fromAnywhere ? "ghost" : "primary"} onClick={markSent}>
                  {dirty ? "I sent my edited version" : "I sent this"}
                </Button>
                {fromAnywhere ? null : (
                  <Button variant="ghost" onClick={copy}>
                    Copy
                  </Button>
                )}
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
