import { useState } from "react";
import { Badge, Button, Card, EmptyState } from "@mimic/ui";
import { CHANNEL_LABELS, type Channel } from "@mimic/contracts";
import { PageHeader } from "@/components/PageHeader";
import { useDeleteSource, useSources, useStartImport } from "@/hooks/useSources";
import { AddSourceDialog } from "./AddSourceDialog";
import { formatRelative } from "@/lib/format";

const STATUS_TONE: Record<string, "success" | "warning" | "danger" | "neutral"> = {
  imported: "success",
  importing: "warning",
  failed: "danger",
  ready: "neutral",
  new: "neutral",
};

export function SourcesPage() {
  const sources = useSources();
  const startImport = useStartImport();
  const remove = useDeleteSource();
  const [adding, setAdding] = useState(false);

  return (
    <div className="stack gap-3">
      <PageHeader
        title="Sources"
        subtitle="Where your messages come from. Everything is read from files you point Mimic at; nothing is uploaded."
        actions={
          <Button variant="primary" onClick={() => setAdding(true)}>
            Add a source
          </Button>
        }
      />

      {sources.isSuccess && sources.data.length === 0 ? (
        <EmptyState
          title="No sources yet"
          body="Export your mail or messages and point Mimic at the file. Only import conversations you own or have permission to process."
          primary={
            <Button variant="primary" onClick={() => setAdding(true)}>
              Add a source
            </Button>
          }
        />
      ) : (
        <Card>
          <table className="table">
            <thead>
              <tr>
                <th>Source</th>
                <th>Channel</th>
                <th className="num">Messages</th>
                <th>Last import</th>
                <th>Status</th>
                <th />
              </tr>
            </thead>
            <tbody>
              {(sources.data ?? []).map((s) => (
                <tr key={s.id}>
                  <td>
                    <div>{s.name}</div>
                    <div className="muted small mono">{s.location ?? s.connector}</div>
                    {s.lastError ? (
                      <div className="danger small">{String(s.lastError.message)}</div>
                    ) : null}
                  </td>
                  <td>{CHANNEL_LABELS[s.channel as Channel] ?? s.channel}</td>
                  <td className="num">{s.messageCount.toLocaleString()}</td>
                  <td className="muted small">{formatRelative(s.lastImportedAt)}</td>
                  <td>
                    <Badge tone={STATUS_TONE[s.status] ?? "neutral"}>{s.status}</Badge>
                  </td>
                  <td>
                    <div className="row gap-1">
                      <Button
                        size="sm"
                        onClick={() => startImport.mutate(s.id)}
                        disabled={s.status === "importing" || startImport.isPending}
                      >
                        {s.messageCount > 0 ? "Import again" : "Import"}
                      </Button>
                      <Button size="sm" variant="ghost" onClick={() => remove.mutate(s.id)}>
                        Remove
                      </Button>
                    </div>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
          <p className="muted small">
            Re-importing the same file adds nothing: messages are identified by the source they came
            from and their own id, so a second run finds only duplicates.
          </p>
        </Card>
      )}

      {adding ? <AddSourceDialog onClose={() => setAdding(false)} /> : null}
    </div>
  );
}
