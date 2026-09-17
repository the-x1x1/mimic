import { useState } from "react";
import { Link, useNavigate } from "react-router-dom";
import { Badge, Button, EmptyState } from "@mimic/ui";
import { Images, Plus } from "lucide-react";
import { PageHeader } from "@/components/PageHeader";
import { useSessions } from "@/hooks/useSessions";
import { useStyles } from "@/hooks/useStyles";
import { formatRelative } from "@/lib/format";
import { NewSessionDialog } from "./NewSessionDialog";
import { SESSION_STATUS_LABEL, sessionStatusTone } from "./sessionStatus";

export function SessionsPage() {
  const sessions = useSessions();
  const styles = useStyles();
  const [open, setOpen] = useState(false);
  const navigate = useNavigate();
  const styleName = new Map((styles.data ?? []).map((s) => [s.id, s.name]));

  return (
    <>
      <PageHeader
        title="Sessions"
        subtitle="A session is a new shoot: Mimic groups it into scenes, predicts edits in your Style, and applies them to Lightroom with a snapshot and read-back on every photo."
        actions={
          <Button variant="primary" icon={<Plus />} onClick={() => setOpen(true)}>
            New Session
          </Button>
        }
      />
      {sessions.isSuccess && sessions.data.length === 0 ? (
        <EmptyState
          icon={<Images />}
          title="No sessions yet"
          body="Point Mimic at a folder of new RAW files or your current Lightroom selection. Ingest and scene grouping work without a trained Style; prediction needs one."
          primary={
            <Button variant="primary" onClick={() => setOpen(true)}>
              Create a Session
            </Button>
          }
        />
      ) : (
        <div className="style-list">
          {(sessions.data ?? []).map((s) => (
            <Link key={s.id} to={`/sessions/${s.id}`} className="style-card">
              <div className="style-card__head">
                <h3>{s.name}</h3>
                <Badge tone={sessionStatusTone(s.status)}>
                  {SESSION_STATUS_LABEL[s.status] ?? s.status}
                </Badge>
              </div>
              <div className="style-card__meta">
                <span>{s.assetCount.toLocaleString()} photos</span>
                <span>
                  {s.activeStyleProfileId
                    ? (styleName.get(s.activeStyleProfileId) ?? "Style")
                    : "no Style chosen"}
                </span>
                <span>created {formatRelative(s.createdAt)}</span>
              </div>
              {s.sourcePath ? <p className="muted small mono">{s.sourcePath}</p> : null}
            </Link>
          ))}
        </div>
      )}
      <NewSessionDialog
        open={open}
        onOpenChange={setOpen}
        onCreated={(id) => navigate(`/sessions/${id}`)}
      />
    </>
  );
}
