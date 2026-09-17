import { useState } from "react";
import { useNavigate, useParams } from "react-router-dom";
import * as Tabs from "@radix-ui/react-tabs";
import { Badge, Button, Card, EmptyState, InlineError } from "@mimic/ui";
import { FolderPlus, Play, RefreshCw, Trash2 } from "lucide-react";
import { PageHeader } from "@/components/PageHeader";
import { DataQualityPanel } from "@/components/DataQualityPanel";
import { PhotoGrid } from "@/components/PhotoGrid";
import { useDeleteStyle, useStyleDetail } from "@/hooks/useStyles";
import { useLibraries, useLibraryAssets, useStartScan } from "@/hooks/useLibraries";
import { formatDate } from "@/lib/format";
import { AddTrainingDataDialog } from "./AddTrainingDataDialog";

export function StyleDetailPage() {
  const { styleId = "" } = useParams();
  const detail = useStyleDetail(styleId);
  const libraries = useLibraries();
  const startScan = useStartScan();
  const del = useDeleteStyle();
  const navigate = useNavigate();
  const [adding, setAdding] = useState(false);
  const [browseLib, setBrowseLib] = useState<string | null>(null);
  const assets = useLibraryAssets(browseLib);

  if (detail.isError)
    return <InlineError title="Style not found">{(detail.error as Error).message}</InlineError>;
  if (!detail.data) return <p className="muted">Loading…</p>;
  const { style, libraries: reports, versions, training } = detail.data;
  const libById = new Map((libraries.data ?? []).map((l) => [l.id, l]));

  return (
    <>
      <PageHeader
        title={style.name}
        subtitle={
          <span className="row gap-2">
            {style.activeVersion ? (
              <Badge tone="success">v{style.activeVersion.semanticVersion} active</Badge>
            ) : (
              <Badge>not trained</Badge>
            )}
            <span className="muted">
              {style.trainingExamples.toLocaleString()} examples · {style.cameras.length} cameras
            </span>
          </span>
        }
        actions={
          <>
            <Button icon={<FolderPlus />} onClick={() => setAdding(true)}>
              Add Training Data
            </Button>
            <Button variant="primary" icon={<Play />} disabled title={training.reason}>
              Train New Version
            </Button>
            <Button
              variant="danger"
              icon={<Trash2 />}
              onClick={() => {
                if (
                  window.confirm(
                    `Delete Style “${style.name}”? Ingested photos and edits are kept.`,
                  )
                )
                  del.mutate(style.id, { onSuccess: () => navigate("/styles") });
              }}
              aria-label="Delete style"
            />
          </>
        }
      />
      <Tabs.Root defaultValue="overview" className="tabs">
        <Tabs.List className="tabs__list" aria-label="Style sections">
          <Tabs.Trigger value="overview" className="tabs__trigger">
            Overview
          </Tabs.Trigger>
          <Tabs.Trigger value="data" className="tabs__trigger">
            Training Data
          </Tabs.Trigger>
          <Tabs.Trigger value="versions" className="tabs__trigger">
            Versions
          </Tabs.Trigger>
          <Tabs.Trigger value="corrections" className="tabs__trigger">
            Corrections
          </Tabs.Trigger>
        </Tabs.List>

        <Tabs.Content value="overview" className="tabs__content">
          {reports.length === 0 ? (
            <EmptyState
              title="No training data attached"
              body="Add a folder or a Lightroom selection to start building this Style's dataset."
              primary={
                <Button variant="primary" onClick={() => setAdding(true)}>
                  Add Training Data
                </Button>
              }
            />
          ) : (
            <div className="stack gap-3">
              {reports.map((r) => (
                <Card
                  key={r.libraryId}
                  title={libById.get(r.libraryId)?.name ?? "Library"}
                  actions={
                    <Button
                      size="sm"
                      icon={<RefreshCw />}
                      onClick={() => startScan.mutate(r.libraryId)}
                    >
                      Rescan
                    </Button>
                  }
                >
                  <DataQualityPanel report={r} />
                </Card>
              ))}
              <InlineError title="Training is not part of this build">
                {training.reason}
              </InlineError>
            </div>
          )}
        </Tabs.Content>

        <Tabs.Content value="data" className="tabs__content">
          <div className="stack gap-3">
            {style.libraryIds.map((id) => {
              const lib = libById.get(id);
              return (
                <Card
                  key={id}
                  title={lib?.name ?? id}
                  actions={
                    <Button size="sm" onClick={() => setBrowseLib(browseLib === id ? null : id)}>
                      {browseLib === id ? "Hide photos" : "Browse photos"}
                    </Button>
                  }
                >
                  <div className="muted small">
                    {lib?.sourceType === "folder_sidecars"
                      ? lib.rootPath
                      : lib?.sourceType === "lightroom_catalog"
                        ? "Lightroom catalog"
                        : "Demo"}{" "}
                    · {lib?.assetCount ?? 0} photos · {lib?.validPairCount ?? 0} edit pairs · last
                    scanned {formatDate(lib?.lastScannedAt)}
                  </div>
                  {browseLib === id ? (
                    assets.data && assets.data.length > 0 ? (
                      <div className="mt-3">
                        <PhotoGrid assets={assets.data} />
                      </div>
                    ) : (
                      <p className="muted small mt-3">
                        {assets.isLoading ? "Loading…" : "No photos ingested yet."}
                      </p>
                    )
                  ) : null}
                </Card>
              );
            })}
          </div>
        </Tabs.Content>

        <Tabs.Content value="versions" className="tabs__content">
          {versions.length === 0 ? (
            <EmptyState
              title="No model versions"
              body="Each training run creates an immutable version with its own holdout metrics. You will be able to activate, compare and roll back versions here once training ships in 0.2.0."
            />
          ) : (
            <ul className="plain-list">
              {versions.map((v) => (
                <li key={v.id}>
                  v{v.semanticVersion} · {v.status}
                  {v.isActive ? " · active" : ""}
                </li>
              ))}
            </ul>
          )}
        </Tabs.Content>

        <Tabs.Content value="corrections" className="tabs__content">
          <EmptyState
            title="No corrections yet"
            body="When you adjust a Mimic-edited photo in Lightroom and sync corrections, the differences appear here and feed the next version (0.4.0)."
          />
        </Tabs.Content>
      </Tabs.Root>
      <AddTrainingDataDialog styleId={style.id} open={adding} onOpenChange={setAdding} />
    </>
  );
}
