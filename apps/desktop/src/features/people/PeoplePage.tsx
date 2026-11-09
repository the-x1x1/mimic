import { useState } from "react";
import { Badge, Button, Card, EmptyState, InlineError } from "@mimic/ui";
import {
  CHANNEL_LABELS,
  MIN_SAMPLE,
  SUGGESTED_RELATIONSHIPS,
  describePeopleList,
  type Channel,
  type ParticipantSummary,
} from "@mimic/contracts";
import { PageHeader } from "@/components/PageHeader";
import { usePeople, useSetRelationship } from "@/hooks/usePeople";
import { DeletePersonDialog } from "./DeletePersonDialog";
import { PersonConversations } from "./PersonConversations";
import { countOf, formatRelative } from "@/lib/format";

/**
 * People, ordered by who the user has heard from or written to most recently.
 * The counts are the point: a relationship profile is only built where there
 * is enough of the user's own writing to build one, and this screen says
 * which those are.
 *
 * Senders whose mail all looks automated — newsletters, notification
 * services, no-reply addresses — are not people, and a connected mailbox has
 * hundreds of them. They are left out, counted, and shown when asked for, so
 * leaving them out never hides anyone; saying how you know one puts them back.
 */
export function PeoplePage() {
  const [showAutomated, setShowAutomated] = useState(false);
  // Two queries, so that loading or failing to list the senders never takes
  // the people, or the way to hide the senders again, off the screen.
  const people = usePeople(false);
  const leftOut = usePeople(true, showAutomated);
  const setRelationship = useSetRelationship();
  const [deleting, setDeleting] = useState<{ id: string; name: string } | null>(null);
  const [reading, setReading] = useState<{ id: string; name: string } | null>(null);
  const view = people.data;
  const senders = leftOut.data?.showingAutomated ? leftOut.data : undefined;

  if (view && view.peopleTotal === 0 && view.automatedSendersTotal === 0) {
    return (
      <EmptyState
        title="I haven’t met anyone yet"
        body="People appear here once you import messages. Mimic works out who is who from the addresses on them."
      />
    );
  }

  const line = view ? describePeopleList(view) : null;

  return (
    <div className="stack gap-3">
      <PageHeader
        title="People"
        subtitle="Everyone who has written to you in what I’ve read. Tell me how you know someone and I’ll write to them differently."
      />
      {people.isError ? <InlineError>{(people.error as Error).message}</InlineError> : null}
      {line ? (
        <p className="muted small">
          {line}{" "}
          {view && view.automatedSendersTotal > 0 ? (
            <button
              type="button"
              className="linkish"
              onClick={() => setShowAutomated((v) => !v)}
              aria-expanded={showAutomated}
              aria-controls={showAutomated ? "automated-senders" : undefined}
            >
              {showAutomated ? "Hide them" : "Show them"}
            </button>
          ) : null}
        </p>
      ) : null}
      {view && view.people.length > 0 ? (
        <ul className="people" aria-label="People">
          {view.people.map((p) => (
            <PersonRow
              key={p.participant.id}
              p={p}
              onSave={setRelationship.mutate}
              onOpen={setReading}
              onDelete={setDeleting}
            />
          ))}
        </ul>
      ) : null}
      {showAutomated && view && view.automatedSendersTotal > 0 ? (
        <section id="automated-senders" aria-label="Senders I left out">
          <Card title="Senders I left out">
            {leftOut.isError ? (
              <InlineError>
                I couldn&rsquo;t list them: {(leftOut.error as Error).message}
              </InlineError>
            ) : !senders ? (
              <p className="muted small">Looking&hellip;</p>
            ) : (
              <>
                <p className="muted small">
                  Everything these sent looks automated to me, going by its headers, and you
                  haven&rsquo;t written to any of them. They aren&rsquo;t used for anything about
                  how you write. If one is a person, say how you know them and they go back among
                  people &mdash; and one ordinary message from them does the same.
                  {senders.automatedSenders.length < senders.automatedSendersTotal
                    ? ` Here are the ${senders.automatedSenders.length} most recent.`
                    : null}
                </p>
                <ul className="people" aria-label="Senders I left out">
                  {senders.automatedSenders.map((p) => (
                    <PersonRow
                      key={p.participant.id}
                      p={p}
                      sender
                      onSave={setRelationship.mutate}
                      onOpen={setReading}
                      onDelete={setDeleting}
                    />
                  ))}
                </ul>
              </>
            )}
          </Card>
        </section>
      ) : null}
      <datalist id="relationships">
        {SUGGESTED_RELATIONSHIPS.map((r) => (
          <option key={r} value={r} />
        ))}
      </datalist>
      {reading ? (
        <PersonConversations
          participantId={reading.id}
          name={reading.name}
          onClose={() => setReading(null)}
        />
      ) : null}
      {deleting ? (
        <DeletePersonDialog
          participantId={deleting.id}
          name={deleting.name}
          onClose={() => setDeleting(null)}
        />
      ) : null}
    </div>
  );
}

/**
 * One person: who, what is known of them, how the user knows them, and what
 * can be done. A row of its own rather than a table's: eight columns were
 * wider than the drawer, and squeezed into it they broke every word.
 */
function PersonRow({
  p,
  sender = false,
  onSave,
  onOpen,
  onDelete,
}: {
  p: ParticipantSummary;
  /** One of the senders left out as automated: no profile is built for them. */
  sender?: boolean;
  onSave: (v: { id: string; relationship: string | null }) => void;
  onOpen: (d: { id: string; name: string }) => void;
  onDelete: (d: { id: string; name: string }) => void;
}) {
  const channels = p.channels.map((c) => CHANNEL_LABELS[c as Channel] ?? c).join(", ");
  return (
    <li className="person">
      <div className="person__top">
        <div className="person__who">
          <Who p={p} />
        </div>
        <div className="person__actions">
          <ConversationsButton p={p} onOpen={onOpen} />
          <DeleteButton p={p} onDelete={onDelete} />
        </div>
      </div>
      <div className="person__facts small">
        <span>{countOf(p.messageCount, "message")}</span>
        {sender ? null : <span>{p.sentByUser.toLocaleString()} from you</span>}
        {channels ? <span>{channels}</span> : null}
        <span className="muted">last {formatRelative(p.lastMessageAt)}</span>
        {sender ? null : p.hasRelationshipProfile ? (
          <Badge tone="success">Profile built</Badge>
        ) : (
          <Badge
            tone="neutral"
            title={`A profile needs at least ${MIN_SAMPLE} messages you wrote to them.`}
          >
            {countOf(Math.max(0, MIN_SAMPLE - p.sentByUser), "more", "more")} needed for a profile
          </Badge>
        )}
      </div>
      <label className="person__how">
        <span className="muted small">How you know them</span>
        <Relationship p={p} onSave={onSave} />
      </label>
    </li>
  );
}

function Relationship({
  p,
  onSave,
}: {
  p: ParticipantSummary;
  onSave: (v: { id: string; relationship: string | null }) => void;
}) {
  return (
    <input
      list="relationships"
      aria-label={`How you know ${p.participant.displayName}`}
      defaultValue={p.participant.relationship ?? ""}
      placeholder="unset"
      onBlur={(e) => {
        const relationship = e.target.value.trim() || null;
        if (relationship !== (p.participant.relationship ?? null)) {
          onSave({ id: p.participant.id, relationship });
        }
      }}
    />
  );
}

function Who({ p }: { p: ParticipantSummary }) {
  return (
    <>
      <div className="person__name">{p.participant.displayName}</div>
      <div className="muted small mono person__ids">
        {p.participant.identifiers.map((i) => i.value).join(", ")}
      </div>
    </>
  );
}

function ConversationsButton({
  p,
  onOpen,
}: {
  p: ParticipantSummary;
  onOpen: (d: { id: string; name: string }) => void;
}) {
  return (
    <Button
      size="sm"
      variant="ghost"
      aria-label={`Conversations with ${p.participant.displayName}`}
      onClick={() => onOpen({ id: p.participant.id, name: p.participant.displayName })}
    >
      Conversations
    </Button>
  );
}

function DeleteButton({
  p,
  onDelete,
}: {
  p: ParticipantSummary;
  onDelete: (d: { id: string; name: string }) => void;
}) {
  return (
    <Button
      size="sm"
      variant="ghost"
      aria-label={`Delete ${p.participant.displayName}`}
      onClick={() => onDelete({ id: p.participant.id, name: p.participant.displayName })}
    >
      Delete
    </Button>
  );
}
