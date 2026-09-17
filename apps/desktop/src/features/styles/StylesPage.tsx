import { useEffect, useState } from "react";
import { Link, useNavigate, useSearchParams } from "react-router-dom";
import { Badge, Button, EmptyState } from "@mimic/ui";
import { Layers, Plus } from "lucide-react";
import { PageHeader } from "@/components/PageHeader";
import { useStyles } from "@/hooks/useStyles";
import { NewStyleDialog } from "./NewStyleDialog";
import { formatRelative } from "@/lib/format";

export function StylesPage() {
  const styles = useStyles();
  const [params, setParams] = useSearchParams();
  const [open, setOpen] = useState(params.get("new") === "1");
  const navigate = useNavigate();

  useEffect(() => {
    if (params.get("new") === "1") {
      setOpen(true);
      params.delete("new");
      setParams(params, { replace: true });
    }
  }, [params, setParams]);

  return (
    <>
      <PageHeader
        title="Styles"
        subtitle="A Style is one editing intent, learned from your past Lightroom edits."
        actions={
          <Button variant="primary" icon={<Plus />} onClick={() => setOpen(true)}>
            New Style
          </Button>
        }
      />
      {styles.isSuccess && styles.data.length === 0 ? (
        <EmptyState
          icon={<Layers />}
          title="No Styles yet"
          body="Create your first Style from a folder of edited RAW files or a Lightroom selection. Mimic will scan the source and show you whether there is enough data to learn from."
          primary={
            <Button variant="primary" onClick={() => setOpen(true)}>
              Create a Style
            </Button>
          }
        />
      ) : (
        <div className="style-list">
          {(styles.data ?? []).map((s) => (
            <Link key={s.id} to={`/styles/${s.id}`} className="style-card">
              <div className="style-card__head">
                <h3>{s.name}</h3>
                {s.activeVersion ? (
                  <Badge tone="success">v{s.activeVersion.semanticVersion}</Badge>
                ) : (
                  <Badge>not trained</Badge>
                )}
              </div>
              <div className="style-card__meta">
                <span>{s.trainingExamples.toLocaleString()} examples</span>
                <span>
                  {s.cameras.length} camera{s.cameras.length === 1 ? "" : "s"}
                </span>
                <span>updated {formatRelative(s.updatedAt)}</span>
              </div>
              {s.description ? <p className="muted small">{s.description}</p> : null}
            </Link>
          ))}
        </div>
      )}
      <NewStyleDialog
        open={open}
        onOpenChange={setOpen}
        onCreated={(id) => navigate(`/styles/${id}`)}
      />
    </>
  );
}
