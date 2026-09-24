import { useId, useState } from "react";
import { Button, InlineError } from "@mimic/ui";
import { yoursAs, type AddressOwner, type WriterName } from "@mimic/contracts";
import { FoldConfirm } from "@/features/identity/AddAddressForm";
import {
  isStaleConfirmation,
  useAddIdentifier,
  useIdentity,
  usePreviewAddress,
  useRemoveIdentifier,
} from "@/hooks/usePeople";
import { countOf } from "@/lib/format";

/** How many writers are shown before the rest are asked for. */
const SHOWN = 8;

/**
 * Which name in a chat is the user's. WhatsApp knows people only by the name
 * the phone saved them under, so the user says which one is them, and the
 * address the import gives that name — exactly as the check reported it —
 * becomes one of theirs. Before it is added: if mail under it was already
 * filed as someone's, the user is shown who and what would move; a second
 * name, or the one the chat is named after (in a chat between two, the other
 * person), is asked about first. Before importing, a name can be taken back.
 * A Discord package is one account's, and is the user's already when the
 * account's email is one of theirs.
 */
export function WhoIsYou({
  names,
  connector = "whatsapp",
}: {
  names: WriterName[];
  /** Which kind of source asks: what it knows people by decides what is said. */
  connector?: string;
}) {
  const identity = useIdentity();
  const preview = usePreviewAddress();
  const add = useAddIdentifier();
  const remove = useRemoveIdentifier();
  const [all, setAll] = useState(false);
  // Addresses said to be the user's here, and whether mail already read
  // under them moved to the user as they were. "Not me" can take back only
  // one where nothing moved: removing an address does not give back mail
  // read under it, whether moved here or read before.
  const [added, setAdded] = useState<Map<string, "clean" | "moved">>(() => new Map());
  const [checking, setChecking] = useState<{ writer: WriterName; why: string } | null>(null);
  const [asking, setAsking] = useState<{
    writer: WriterName;
    owner: AddressOwner;
    changed: boolean;
  } | null>(null);
  const heading = useId();
  const working = preview.isPending || add.isPending || remove.isPending;
  const error = add.error ?? preview.error ?? remove.error;

  const identifiers = identity.data?.identifiers ?? [];
  const mineAs = (w: WriterName) => yoursAs(w, identifiers);
  const yours = names.filter((w) => mineAs(w) !== null);
  const shown = all ? names : names.slice(0, SHOWN);

  async function ask(writer: WriterName, changed: boolean) {
    const seen = await preview.mutateAsync({ kind: writer.kind, value: writer.value });
    if (seen.alreadyYours) {
      setAsking(null);
      return;
    }
    if (seen.owner) {
      setAsking({ writer, owner: seen.owner, changed });
      return;
    }
    await commit(writer, null);
  }

  async function commit(writer: WriterName, owner: AddressOwner | null) {
    try {
      await add.mutateAsync({ kind: writer.kind, value: writer.value, confirmedOwner: owner });
      setAdded((a) =>
        new Map(a).set(`${writer.kind}:${writer.normalized}`, owner === null ? "clean" : "moved"),
      );
      setAsking(null);
    } catch (e) {
      // What was shown is not what is there now: ask again.
      if (isStaleConfirmation(e)) {
        add.reset();
        await ask(writer, true);
      }
    }
  }

  const quietly = (p: Promise<unknown>) =>
    void p.catch(() => {
      // Shown below, from the mutation's own error.
    });

  function pick(writer: WriterName) {
    add.reset();
    preview.reset();
    remove.reset();
    // Someone else's words read as the user's is the mistake that matters,
    // so a pick that is likely one is asked about first.
    if (writer.chatNamedAfter) {
      setChecking({
        writer,
        why: `This chat is named after ${writer.name}. In a chat between two people, that’s the other person.`,
      });
    } else if (yours.length > 0) {
      setChecking({
        writer,
        why: `You’ve said ${yours.map((w) => w.name).join(" and ")} is you. Only say ${writer.name} is too if you write under two names — from two phones, say.`,
      });
    } else {
      quietly(ask(writer, false));
    }
  }

  return (
    <div className="stack gap-1" role="group" aria-labelledby={heading}>
      {connector === "discord" ? (
        yours.length > 0 ? (
          <p id={heading}>
            <strong>This is you.</strong> A Discord package is everything one account wrote, and
            this account is one of yours.
          </p>
        ) : (
          <p id={heading}>
            <strong>Is this you?</strong> A Discord package is everything one account wrote, so
            I&rsquo;ll import it once you say it&rsquo;s yours.
          </p>
        )
      ) : (
        <>
          <p id={heading}>
            <strong>Which of these is you?</strong> WhatsApp knows people only by the name your
            phone saved them under, so I need you to say which one wrote your messages. Until you
            do, I&rsquo;ll read them as someone else&rsquo;s.
          </p>
          <p className="muted small">
            Someone saved on your phone under exactly your name would be read as you too.
          </p>
        </>
      )}
      {yours.length > 0 ? (
        <p className="small">
          I&rsquo;ll read what {yours.map((w) => w.name).join(" and ")} wrote as yours.
        </p>
      ) : null}
      <ul className="plain-list stack gap-1">
        {shown.map((w) => {
          const as = mineAs(w);
          return (
            <li key={`${w.kind}:${w.normalized}`} className="row gap-2">
              <span>{w.name}</span>
              <span className="muted small">{countOf(w.messages, "message")}</span>
              {as && added.get(`${w.kind}:${w.normalized}`) === "moved" ? (
                <span className="small">You</span>
              ) : as && !added.has(`${w.kind}:${w.normalized}`) ? (
                <span className="small">You — already one of your addresses</span>
              ) : as ? (
                <>
                  <span className="small">You</span>
                  <Button
                    size="sm"
                    variant="ghost"
                    aria-label={`Not me: ${w.name}`}
                    disabled={working}
                    onClick={() => quietly(remove.mutateAsync(as.id))}
                  >
                    Not me
                  </Button>
                </>
              ) : (
                <Button
                  size="sm"
                  variant="ghost"
                  aria-label={`That’s me: ${w.name}`}
                  disabled={working || asking !== null || checking !== null || identity.isPending}
                  onClick={() => pick(w)}
                >
                  That&rsquo;s me
                </Button>
              )}
            </li>
          );
        })}
      </ul>
      {names.length > SHOWN ? (
        <button type="button" className="linkish" onClick={() => setAll((a) => !a)}>
          {all ? "Show only the most frequent" : `Show all ${names.length} names`}
        </button>
      ) : null}
      {checking ? (
        <div
          className="fold-confirm stack gap-2"
          role="group"
          aria-label={`Is ${checking.writer.name} you?`}
        >
          <strong>Is {checking.writer.name} you?</strong>
          <p>{checking.why}</p>
          <div className="row gap-2">
            <Button
              variant="primary"
              autoFocus
              onClick={() => {
                const writer = checking.writer;
                setChecking(null);
                quietly(ask(writer, false));
              }}
            >
              Yes, that&rsquo;s me
            </Button>
            <Button variant="ghost" onClick={() => setChecking(null)}>
              No
            </Button>
          </div>
        </div>
      ) : null}
      {asking ? (
        <FoldConfirm
          address={asking.writer.value}
          owner={asking.owner}
          pending={add.isPending}
          changed={asking.changed}
          focus
          onYes={() => quietly(commit(asking.writer, asking.owner))}
          onNo={() => setAsking(null)}
        />
      ) : null}
      {error ? <InlineError>{error.message}</InlineError> : null}
    </div>
  );
}
