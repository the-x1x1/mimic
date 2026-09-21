import { useState } from "react";
import { Badge, Button, Card, EmptyState } from "@mimic/ui";
import {
  CHANNEL_LABELS,
  MIN_SAMPLE,
  SUGGESTED_RELATIONSHIPS,
  type Channel,
} from "@mimic/contracts";
import { PageHeader } from "@/components/PageHeader";
import { usePeople, useSetRelationship } from "@/hooks/usePeople";
import { DeletePersonDialog } from "./DeletePersonDialog";
import { countOf, formatRelative } from "@/lib/format";

/**
 * People, ordered by who the user has spoken to most recently. The counts are
 * the point: a relationship profile is only built where there is enough of the
 * user's own writing to build one, and this screen says which those are.
 */
export function PeoplePage() {
  const people = usePeople();
  const setRelationship = useSetRelationship();
  const [deleting, setDeleting] = useState<{ id: string; name: string } | null>(null);

  if (people.isSuccess && people.data.length === 0) {
    return (
      <EmptyState
        title="I haven’t met anyone yet"
        body="People appear here once you import messages. Mimic works out who is who from the addresses on them."
      />
    );
  }

  return (
    <div className="stack gap-3">
      <PageHeader
        title="People"
        subtitle="Everyone I’ve seen you write to. Tell me how you know someone and I’ll write to them differently."
      />
      <Card>
        <table className="table">
          <thead>
            <tr>
              <th>Person</th>
              <th>How you know them</th>
              <th className="num">Messages</th>
              <th className="num">Yours</th>
              <th>Channels</th>
              <th>Last</th>
              <th>Profile</th>
              <th />
            </tr>
          </thead>
          <tbody>
            {(people.data ?? []).map((p) => (
              <tr key={p.participant.id}>
                <td>
                  <div>{p.participant.displayName}</div>
                  <div className="muted small mono">
                    {p.participant.identifiers.map((i) => i.value).join(", ")}
                  </div>
                </td>
                <td>
                  <input
                    list="relationships"
                    defaultValue={p.participant.relationship ?? ""}
                    placeholder="unset"
                    onBlur={(e) =>
                      setRelationship.mutate({
                        id: p.participant.id,
                        relationship: e.target.value.trim() || null,
                      })
                    }
                  />
                </td>
                <td className="num">{p.messageCount.toLocaleString()}</td>
                <td className="num">{p.sentByUser.toLocaleString()}</td>
                <td>{p.channels.map((c) => CHANNEL_LABELS[c as Channel] ?? c).join(", ")}</td>
                <td className="muted small">{formatRelative(p.lastMessageAt)}</td>
                <td>
                  {p.hasRelationshipProfile ? (
                    <Badge tone="success">Built</Badge>
                  ) : (
                    <Badge
                      tone="neutral"
                      title={`A profile needs at least ${MIN_SAMPLE} messages you wrote to them.`}
                    >
                      {countOf(Math.max(0, MIN_SAMPLE - p.sentByUser), "more")} needed
                    </Badge>
                  )}
                </td>
                <td>
                  <Button
                    size="sm"
                    variant="ghost"
                    onClick={() =>
                      setDeleting({ id: p.participant.id, name: p.participant.displayName })
                    }
                  >
                    Delete
                  </Button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
        <datalist id="relationships">
          {SUGGESTED_RELATIONSHIPS.map((r) => (
            <option key={r} value={r} />
          ))}
        </datalist>
      </Card>
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
