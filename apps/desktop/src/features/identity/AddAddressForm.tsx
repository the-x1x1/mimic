import { useEffect, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { Button, InlineError } from "@mimic/ui";
import {
  IDENTIFIER_LABELS,
  IdentifierKind,
  describeFold,
  describeSentFolder,
  type AddressAdded,
  type AddressOwner,
  type HeldAddress,
  type SentFolderPerson,
} from "@mimic/contracts";
import { qk } from "@/app/queryClient";
import {
  isStaleConfirmation,
  useAddIdentifier,
  useClaimHeldAddress,
  useKeepApart,
  usePreviewAddress,
  useSentFolderPeople,
} from "@/hooks/usePeople";

/** Said when the question had to be asked again because its answer changed. */
const CHANGED = "That changed since I asked, so here it is again as it is now.";

/** The person asked about no longer exists. */
function isGone(e: unknown): boolean {
  return (e as { code?: unknown } | null)?.code === "not_found";
}

/**
 * The question asked before an address filed under someone becomes the
 * user's. The lines come from the preview, and the fold refuses unless it is
 * handed back exactly what the preview found — the same query, run again in
 * the same transaction as the fold — so what it says is what happens.
 */
export function FoldConfirm({
  address,
  owner,
  pending,
  changed = false,
  focus = false,
  onYes,
  onNo,
}: {
  address: string;
  owner: AddressOwner;
  pending: boolean;
  /** The user said yes to an earlier version of this question. */
  changed?: boolean;
  /** Take the keyboard to the answer when the question appears where it may not be noticed. */
  focus?: boolean;
  onYes: () => void;
  onNo: () => void;
}) {
  const { question, lines } = describeFold(owner, address);
  return (
    <div className="fold-confirm stack gap-2" role="group" aria-label={question}>
      <strong>{question}</strong>
      {changed ? <p className="warn small">{CHANGED}</p> : null}
      <ul className="plain-list">
        {lines.map((line) => (
          <li key={line}>{line}</li>
        ))}
      </ul>
      <div className="row gap-2">
        <Button variant="primary" disabled={pending} autoFocus={focus} onClick={onYes}>
          {pending ? "Moving them over…" : "Yes, that's me"}
        </Button>
        <Button variant="ghost" disabled={pending} onClick={onNo}>
          No
        </Button>
      </div>
    </div>
  );
}

/**
 * Adding an address of the user's, wherever it is done. It asks first what
 * the address would change: if mail already read from it is filed under
 * someone, the user is shown who and what would move, and only a yes adds it.
 * Otherwise it is added straight away.
 */
export function AddAddressForm({
  inputLabel = "Your address",
  placeholder = "you@example.com",
  submitLabel = "Add",
  primary = false,
  autoFocus = false,
  disabled = false,
  onAdded,
}: {
  inputLabel?: string;
  placeholder?: string;
  submitLabel?: string;
  primary?: boolean;
  autoFocus?: boolean;
  disabled?: boolean;
  /**
   * After an add, with what it did, the address as typed, and whether the
   * user was asked about someone first — in which case a result with nothing
   * moved means someone else moved it meanwhile, not that nothing was there.
   */
  onAdded?: (added: AddressAdded, address: string, asked: boolean) => void;
}) {
  const qc = useQueryClient();
  const preview = usePreviewAddress();
  const add = useAddIdentifier();
  const keepApart = useKeepApart();
  const [kind, setKind] = useState<IdentifierKind>("email");
  const [value, setValue] = useState("");
  const [note, setNote] = useState<string | null>(null);
  const [asking, setAsking] = useState<{
    kind: IdentifierKind;
    address: string;
    owner: AddressOwner;
    changed: boolean;
    /** The address is the user's already; only the person under it is in question. */
    alreadyYours: boolean;
  } | null>(null);
  const working = preview.isPending || add.isPending || keepApart.isPending;
  const error = add.error ?? preview.error ?? keepApart.error;

  /**
   * Ask what adding it would do, then add it or put the question. `changed`
   * when this is the second time, because the first answer went stale.
   */
  async function ask(k: IdentifierKind, address: string, changed: boolean) {
    const seen = await preview.mutateAsync({ kind: k, value: address });
    if (seen.owner) {
      setAsking({ kind: k, address, owner: seen.owner, changed, alreadyYours: seen.alreadyYours });
      return;
    }
    setAsking(null);
    if (seen.alreadyYours) {
      setValue("");
      setNote(`${address} is already one of yours.`);
      return;
    }
    await commit(k, address, null);
  }

  async function commit(k: IdentifierKind, address: string, owner: AddressOwner | null) {
    try {
      const added = await add.mutateAsync({ kind: k, value: address, confirmedOwner: owner });
      setAsking(null);
      setValue("");
      onAdded?.(added, address, owner !== null);
    } catch (e) {
      // What was shown is not what is there now: ask again rather than leave
      // a question whose answer would do something else.
      if (isStaleConfirmation(e)) {
        add.reset();
        await ask(k, address, true);
      }
    }
  }

  async function submit() {
    const address = value.trim();
    if (!address) return;
    add.reset();
    setNote(null);
    try {
      await ask(kind, address, false);
    } catch {
      // Shown below, from the mutation's own error.
    }
  }

  /**
   * No. For an address not yet the user's, nothing was added and there is
   * nothing to remember. For one already theirs, the person under it is not
   * them, and that is kept — as the same answer in Settings keeps it.
   */
  async function no() {
    if (!asking) return;
    add.reset();
    if (asking.alreadyYours) {
      try {
        await keepApart.mutateAsync(asking.owner.participantId);
      } catch (e) {
        // Gone already — folded in by something that finished meanwhile —
        // leaves nothing to keep apart.
        if (!isGone(e)) return;
        keepApart.reset();
        void qc.invalidateQueries({ queryKey: qk.identity });
      }
    }
    setAsking(null);
  }

  async function confirm() {
    if (!asking) return;
    try {
      await commit(asking.kind, asking.address, asking.owner);
    } catch {
      // Shown below; the question stays so the user can try again.
    }
  }

  return (
    <div className="stack gap-2">
      <div className="row gap-2">
        <select
          aria-label="Kind of address"
          value={kind}
          disabled={asking !== null}
          onChange={(e) => setKind(e.target.value as IdentifierKind)}
        >
          {IdentifierKind.options.map((k) => (
            <option key={k} value={k}>
              {IDENTIFIER_LABELS[k]}
            </option>
          ))}
        </select>
        <input
          aria-label={inputLabel}
          value={value}
          placeholder={placeholder}
          autoFocus={autoFocus}
          disabled={asking !== null}
          onChange={(e) => setValue(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && !working && !disabled) void submit();
          }}
        />
        <Button
          variant={primary ? "primary" : "secondary"}
          disabled={disabled || working || asking !== null || !value.trim()}
          onClick={() => void submit()}
        >
          {submitLabel}
        </Button>
      </div>
      {asking ? (
        <FoldConfirm
          address={asking.address}
          owner={asking.owner}
          pending={add.isPending || keepApart.isPending}
          changed={asking.changed}
          onYes={() => void confirm()}
          onNo={() => void no()}
        />
      ) : null}
      {note ? <p className="muted small">{note}</p> : null}
      {error ? <InlineError>{error.message}</InlineError> : null}
    </div>
  );
}

/**
 * One of the user's addresses that mail already read is still filed under
 * someone by, with the same question adding it would have asked. Settings
 * shows one per held address.
 *
 * What the question shows is kept as it was when it opened, and that is what
 * a yes sends: the list is read again after every job, and a question that
 * changed under the user's eyes would have them agree to something they did
 * not read. When the list changes while it is open, the question shows the
 * new version and says so.
 */
export function HeldAddressQuestion({ held }: { held: HeldAddress }) {
  const qc = useQueryClient();
  const claim = useClaimHeldAddress();
  const keepApart = useKeepApart();
  const [shown, setShown] = useState<AddressOwner | null>(null);
  const [changed, setChanged] = useState(false);
  const { identifier, owner, keptApart } = held;
  const n = owner.messages;

  useEffect(() => {
    if (shown && JSON.stringify(shown) !== JSON.stringify(owner)) {
      setShown(owner);
      setChanged(true);
    }
  }, [owner, shown]);

  if (!shown) {
    const filed =
      n === 0
        ? "."
        : n === 1
          ? ", with one message filed under them."
          : `, with ${n.toLocaleString()} messages filed under them.`;
    return (
      <p className="muted small row gap-2">
        <span>
          {keptApart
            ? `You said ${owner.displayName} isn't you, so what I'd already read from it stays theirs; what's sent from it now counts as yours. If it isn't your address, remove it.`
            : `I still have it down as ${owner.displayName}'s${filed}`}
        </span>
        <Button size="sm" variant="ghost" onClick={() => setShown(owner)}>
          {keptApart ? "Is that you after all?" : "Is that you?"}
        </Button>
      </p>
    );
  }

  const close = () => {
    claim.reset();
    setChanged(false);
    setShown(null);
  };

  return (
    <div className="stack gap-2">
      <FoldConfirm
        address={identifier.value}
        owner={shown}
        pending={claim.isPending || keepApart.isPending}
        changed={changed}
        onYes={async () => {
          try {
            await claim.mutateAsync({ identifierId: identifier.id, owner: shown });
          } catch (e) {
            // The list is read again, and the effect above puts the new
            // version in front of the user.
            if (isStaleConfirmation(e)) {
              claim.reset();
              await qc.invalidateQueries({ queryKey: qk.heldAddresses });
            }
          }
        }}
        onNo={async () => {
          if (keptApart) {
            close();
            return;
          }
          try {
            await keepApart.mutateAsync(shown.participantId);
            close();
          } catch (e) {
            // Gone already — folded in by something that finished meanwhile —
            // leaves nothing to keep apart; the list is read again.
            if (isGone(e)) {
              keepApart.reset();
              close();
              await qc.invalidateQueries({ queryKey: qk.identity });
            }
          }
        }}
      />
      {claim.error ? <InlineError>{claim.error.message}</InlineError> : null}
      {keepApart.error ? <InlineError>{keepApart.error.message}</InlineError> : null}
    </div>
  );
}

/**
 * Someone whose mail was in the user's Sent folder, under an address that is
 * not the user's yet: what was found, and the question adding that address
 * would ask. A yes adds it with the owner that was shown, so it folds exactly
 * what the question said; a no is kept, and they are not asked about again.
 *
 * As with a held address, what the question shows is kept as it was when it
 * opened: the list is read again after every job, and a question that changed
 * under the user's eyes would have them agree to something they did not read.
 * When it changes while open, the question shows the new version and says so.
 */
export function SentFolderQuestion({
  person,
  open = false,
}: {
  person: SentFolderPerson;
  /** Open from the start, where it is the thing the screen is for. */
  open?: boolean;
}) {
  const qc = useQueryClient();
  const add = useAddIdentifier();
  const keepApart = useKeepApart();
  const [shown, setShown] = useState<AddressOwner | null>(open ? person.owner : null);
  const [changed, setChanged] = useState(false);
  // Answered: the list read again leaves them out, and until it has, the
  // question isn't asked a second time.
  const [answered, setAnswered] = useState(false);
  const { owner } = person;
  const said = describeSentFolder(person);

  useEffect(() => {
    if (shown && JSON.stringify(shown) !== JSON.stringify(owner)) {
      setShown(owner);
      setChanged(true);
    }
  }, [owner, shown]);

  if (answered) return null;
  if (!shown) {
    return (
      <p className="muted small row gap-2 wrap">
        <span>{said.line}</span>
        <Button size="sm" variant="ghost" onClick={() => setShown(owner)}>
          Is that you?
        </Button>
      </p>
    );
  }

  return (
    <div className="stack gap-2">
      <p className="neutral">{said.why}</p>
      <FoldConfirm
        address={person.address}
        owner={shown}
        pending={add.isPending || keepApart.isPending}
        changed={changed}
        onYes={async () => {
          try {
            await add.mutateAsync({
              kind: person.kind,
              value: person.address,
              confirmedOwner: shown,
            });
            setAnswered(true);
          } catch (e) {
            // The list is read again, and the effect above puts the new
            // version in front of the user.
            if (isStaleConfirmation(e)) {
              add.reset();
              await qc.invalidateQueries({ queryKey: qk.sentFolderPeople });
            }
          }
        }}
        onNo={async () => {
          try {
            await keepApart.mutateAsync(shown.participantId);
            setAnswered(true);
          } catch (e) {
            // Gone already — folded in by something that finished meanwhile —
            // leaves nothing to keep apart; the list is read again.
            if (isGone(e)) {
              keepApart.reset();
              setAnswered(true);
              await qc.invalidateQueries({ queryKey: qk.identity });
            }
          }
        }}
      />
      {add.error ? <InlineError>{add.error.message}</InlineError> : null}
      {keepApart.error ? <InlineError>{keepApart.error.message}</InlineError> : null}
    </div>
  );
}

/**
 * The likeliest of the people whose mail was in the user's Sent folder, for
 * the home screen and the import step: one question at a time, the next once
 * it is answered. Settings lists them all.
 *
 * The person asked about stays the one shown until they are answered, even if
 * the list is read again in another order meanwhile — after a refused yes,
 * say — so the question the user was reading doesn't turn into someone else's.
 */
export function SentFolderNotice({ open = false }: { open?: boolean }) {
  const people = useSentFolderPeople();
  const [asking, setAsking] = useState<string | null>(null);
  const list = people.data ?? [];
  const person = list.find((p) => p.owner.participantId === asking) ?? list[0];
  const id = person?.owner.participantId ?? null;

  useEffect(() => {
    if (id !== asking) setAsking(id);
  }, [id, asking]);

  if (!person) return null;
  // Keyed by the person: an answer that removes them shows the next one fresh.
  return <SentFolderQuestion key={person.owner.participantId} person={person} open={open} />;
}
