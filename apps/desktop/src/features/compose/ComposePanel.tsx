import { useCallback, useEffect, useMemo, useRef, useState } from "react";
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
  useWriteAnother,
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
 *
 * Shorter, longer, more casual and more professional write another way of
 * saying the draft beside it (0.10.0-alpha.98), as on the home screen, rather
 * than in its place: the user picks which one is in the editor, and the one
 * used — sent, pasted or copied away — decides the rest (`Db::resolve_draft`),
 * so which way was chosen is learned from (0.10.0-alpha.97). Regenerate and
 * a new message replace the whole set; "None of these" puts it all aside.
 */
export function ComposePanel({ title = "Write something new" }: { title?: string }) {
  const people = usePeople();
  const providers = useProviderState();
  const generate = useGenerateDraft();
  const another = useWriteAnother();
  const resolve = useResolveDraft();

  const [participantId, setParticipantId] = useState<string>("");
  const [channel, setChannel] = useState<Channel>("email");
  const [incoming, setIncoming] = useState("");
  const [intent, setIntent] = useState("");
  // The set being chosen from (0.10.0-alpha.98): the first draft, the other
  // ways of saying it written beside it (oldest first), which one is in the
  // editor (the first when none), and what was typed into each.
  const [first, setFirst] = useState<Draft | null>(null);
  const [ways, setWays] = useState<Draft[]>([]);
  const [chosenId, setChosenId] = useState<string | null>(null);
  const [texts, setTexts] = useState<Record<string, string>>({});
  const draft = first === null ? null : (ways.find((w) => w.id === chosenId) ?? first);
  const edited = draft ? (texts[draft.id] ?? draft.generatedText) : "";
  const setEdited = (text: string) => {
    if (draft) setTexts((t) => ({ ...t, [draft.id]: text }));
  };
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
  // The set's first draft: what is put aside decides the rest with it.
  const current = useRef<Draft | null>(null);
  useEffect(() => {
    current.current = first;
  }, [first]);
  const resetWriteAnother = another.reset;
  /** Show this first draft alone, or nothing. */
  const showSet = useCallback(
    (d: Draft | null) => {
      setFirst(d);
      setWays([]);
      setChosenId(null);
      setTexts({});
      resetWriteAnother();
    },
    [resetWriteAnother],
  );
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
    showSet(null);
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
  }, [location.key, pasted, windowTitle, resolveAsync, showSet]);

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
  // Another way of saying it, shorter, longer or in another register, is not
  // read for habits: what is changed in it was changed from that request.
  const adjusted = draft ? adjustmentOf(draft) : null;

  // Which message is on screen now: a draft finished after another was
  // copied in belongs to the one before (0.10.0-alpha.87).
  const showing = useRef(location.key);
  useEffect(() => {
    showing.current = location.key;
  }, [location.key]);

  /** A first draft, in place of the set on screen. */
  async function run() {
    const writtenFor = showing.current;
    const result = await generate.mutateAsync({ ...request, adjustment: null });
    if (showing.current !== writtenFor) {
      // Its message is gone from here: not shown under the next one, and
      // not left waiting either.
      await resolve
        .mutateAsync({ draftId: result.id, outcome: "regenerated", finalText: null })
        .catch(() => undefined);
      return;
    }
    // The set it replaces was passed over for this one, not left pending for
    // ever: recorded as regenerated, which is neither sent nor turned down,
    // and which the first draft's other ways go with.
    if (first && first.outcome === null) {
      await resolve
        .mutateAsync({ draftId: first.id, outcome: "regenerated", finalText: null })
        .catch(() => undefined);
    }
    showSet(result);
  }

  /**
   * Another way of saying the first draft, written beside it and put in the
   * editor (0.10.0-alpha.98). One finished after the set was replaced is
   * passed over with it: kept nowhere, since nothing is left to choose it
   * from.
   */
  async function writeAnother(adjustment: Adjustment) {
    if (!first) return;
    const forFirst = first.id;
    try {
      const d = await another.mutateAsync({ draftId: forFirst, adjustment });
      if (current.current?.id !== forFirst) return;
      setWays((w) => [...w, d]);
      setChosenId(d.id);
    } catch {
      // Said below, from the mutation's own error.
    }
  }

  /** Not this way: put aside on its own, and the first back in the editor. */
  async function dropWay(d: Draft) {
    await resolve.mutateAsync({ draftId: d.id, outcome: "discarded", finalText: null });
    setWays((w) => w.filter((x) => x.id !== d.id));
    setChosenId((c) => (c === d.id ? null : c));
  }

  const labelOf = (d: Draft) => {
    const a = adjustmentOf(d);
    return a ? ADJUSTMENT_LABELS[a] : "Another way";
  };
  // While another way is being written, nothing in the set is used or put
  // aside (0.10.0-alpha.100): one finished after it would be counted as
  // passed over without the user ever seeing it.
  const writingAnother = another.isPending;
  const offered = new Set(ways.map(adjustmentOf));

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
    if (incoming !== pasted || first !== null) return;
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
    first,
    windowTitle,
    guessDone,
    readiness.ready,
    generate.isPending,
  ]);

  // A draft for a message from anywhere puts the keyboard on putting it
  // back: Enter pastes it (and sends it, when that is on).
  const pasteButton = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    if (first && fromAnywhere) pasteButton.current?.focus();
  }, [first, fromAnywhere]);

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
        ? `You chose the ${ADJUSTMENT_LABELS[adjusted].toLowerCase()} way, and I noticed. What you change in it isn't read for your habits.`
        : dirty
          ? undefined
          : "Nothing to learn from this one — you sent it as written.",
    );
    showSet(null);
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
    if (!draft || !first) return;
    const text = edited;
    setCopiedAway({
      draftId: draft.id,
      firstId: first.id,
      text,
      generatedText: draft.generatedText,
    });
    showSet(null);
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
    if (!draft || !first) return;
    await navigator.clipboard.writeText(edited);
    setCopiedAway({
      draftId: draft.id,
      firstId: first.id,
      text: edited,
      generatedText: draft.generatedText,
    });
    showSet(null);
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
    // Another way that didn't go leaves its first draft and the rest beside
    // it: none of them went either.
    const firstId = copiedAway.firstId;
    if (!went && firstId && firstId !== copiedAway.draftId) {
      await resolve
        .mutateAsync({ draftId: firstId, outcome: "discarded", finalText: null })
        .catch(() => undefined);
    }
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
            </div>
            {generate.isError ? (
              <InlineError>{(generate.error as Error).message}</InlineError>
            ) : null}
          </Card>

          {draft && first ? (
            <Card title="Draft">
              {ways.length > 0 ? (
                <fieldset className="compose__ways">
                  <legend className="muted small">Which way of saying it</legend>
                  <label className="row gap-1">
                    <input
                      type="radio"
                      name="compose-way"
                      checked={draft.id === first.id}
                      onChange={() => setChosenId(null)}
                    />
                    <span>As I first wrote it</span>
                  </label>
                  {ways.map((w) => (
                    <label key={w.id} className="row gap-1">
                      <input
                        type="radio"
                        name="compose-way"
                        checked={draft.id === w.id}
                        onChange={() => setChosenId(w.id)}
                      />
                      <span>{labelOf(w)}</span>
                    </label>
                  ))}
                </fieldset>
              ) : null}
              <textarea
                className="compose__draft"
                rows={8}
                value={edited}
                onChange={(e) => setEdited(e.target.value)}
              />
              {offered.size < Adjustment.options.length ? (
                <div className="thread__another" role="group" aria-label="Write it another way">
                  <span className="muted small" aria-hidden="true">
                    Write it another way:
                  </span>
                  {Adjustment.options
                    .filter((a) => !offered.has(a))
                    .map((a) => (
                      <Button
                        key={a}
                        size="sm"
                        variant="ghost"
                        onClick={() => void writeAnother(a)}
                        disabled={another.isPending || generate.isPending || !readiness.ready}
                      >
                        {ADJUSTMENT_LABELS[a]}
                      </Button>
                    ))}
                  {another.isPending ? (
                    <span className="muted small" role="status">
                      Writing it another way&hellip;
                    </span>
                  ) : null}
                </div>
              ) : null}
              {another.isError ? (
                <InlineError>{(another.error as Error).message}</InlineError>
              ) : null}
              <div className="row gap-2">
                {fromAnywhere && pressEnter ? (
                  <Button
                    ref={pasteButton}
                    variant="primary"
                    disabled={writingAnother}
                    onClick={() => void pasteThere(true)}
                  >
                    Paste it there and send
                  </Button>
                ) : null}
                {fromAnywhere ? (
                  <>
                    <Button
                      ref={pressEnter ? undefined : pasteButton}
                      variant={pressEnter ? "ghost" : "primary"}
                      disabled={writingAnother}
                      onClick={() => void pasteThere()}
                    >
                      Paste it there
                    </Button>
                    <Button
                      variant="ghost"
                      disabled={writingAnother}
                      onClick={() => void copyAndGoBack()}
                    >
                      Copy and go back
                    </Button>
                  </>
                ) : null}
                <Button
                  variant={fromAnywhere ? "ghost" : "primary"}
                  disabled={writingAnother}
                  onClick={markSent}
                >
                  {dirty ? "I sent my edited version" : "I sent this"}
                </Button>
                {fromAnywhere ? null : (
                  <Button variant="ghost" onClick={copy}>
                    Copy
                  </Button>
                )}
                {draft.id !== first.id ? (
                  <Button
                    variant="ghost"
                    disabled={writingAnother}
                    onClick={() => void dropWay(draft).catch(() => undefined)}
                  >
                    Not this one
                  </Button>
                ) : null}
                <Button
                  variant="ghost"
                  disabled={writingAnother}
                  onClick={async () => {
                    // The first draft, which puts every other way aside with it.
                    await resolve.mutateAsync({
                      draftId: first.id,
                      outcome: "discarded",
                      finalText: null,
                    });
                    showSet(null);
                  }}
                >
                  {ways.length > 0 ? "None of these" : "Discard"}
                </Button>
              </div>
              <p className="muted small">
                {adjusted
                  ? `Using the ${ADJUSTMENT_LABELS[adjusted].toLowerCase()} way tells me which way you like; what you change in it isn't read for your habits, since it was written that way on purpose. Nothing is sent for you.`
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
