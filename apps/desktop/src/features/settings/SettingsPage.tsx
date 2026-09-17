import { useEffect, useMemo, useState } from "react";
import { useSearchParams } from "react-router-dom";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Badge, Button, Card, Field, InlineError, Metric } from "@mimic/ui";
import { Copy, FolderOpen, RefreshCw } from "lucide-react";
import type { Settings } from "@mimic/contracts";
import { PageHeader } from "@/components/PageHeader";
import { LightroomSetup } from "@/components/LightroomSetup";
import { CapabilitySummary } from "@/components/CapabilitySummary";
import { useAppInfo, useSettings, useSystemStatus } from "@/hooks/useSystem";
import { useCapabilityMatrix } from "@/hooks/useLightroom";
import { ipc } from "@/lib/ipc";
import { qk } from "@/app/queryClient";
import { toast } from "@/state/toast";
import { formatBytes, formatDate, formatRelative } from "@/lib/format";
import { useUpdater } from "@/features/updater/useUpdater";

const SECTIONS = [
  "general",
  "lightroom",
  "performance",
  "storage",
  "privacy",
  "updates",
  "diagnostics",
] as const;
type Section = (typeof SECTIONS)[number];
const LABELS: Record<Section, string> = {
  general: "General",
  lightroom: "Lightroom",
  performance: "Performance",
  storage: "Storage",
  privacy: "Privacy",
  updates: "Updates",
  diagnostics: "Diagnostics",
};

export function SettingsPage() {
  const [params, setParams] = useSearchParams();
  const initial = (params.get("section") as Section) || "general";
  const [section, setSection] = useState<Section>(SECTIONS.includes(initial) ? initial : "general");
  useEffect(() => {
    const s = params.get("section") as Section | null;
    if (s && SECTIONS.includes(s)) setSection(s);
  }, [params]);
  const select = (s: Section) => {
    setSection(s);
    setParams({ section: s }, { replace: true });
  };
  return (
    <>
      <PageHeader title="Settings" />
      <div className="settings">
        <nav className="settings__nav" aria-label="Settings sections">
          {SECTIONS.map((s) => (
            <button
              key={s}
              className={s === section ? "settings__link settings__link--on" : "settings__link"}
              onClick={() => select(s)}
            >
              {LABELS[s]}
            </button>
          ))}
        </nav>
        <div className="settings__body">
          {section === "general" && <GeneralSection />}
          {section === "lightroom" && <LightroomSection />}
          {section === "performance" && <PerformanceSection />}
          {section === "storage" && <StorageSection />}
          {section === "privacy" && <PrivacySection />}
          {section === "updates" && <UpdatesSection />}
          {section === "diagnostics" && <DiagnosticsSection />}
        </div>
      </div>
    </>
  );
}

function useSetSetting() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (args: { key: keyof Settings; value: unknown }) =>
      ipc.setSetting(args.key, args.value),
    onSuccess: (data) => qc.setQueryData(qk.settings, data),
    onError: (e: Error) => toast.danger("Setting not saved", e.message),
  });
}

function Toggle({
  id,
  label,
  hint,
  checked,
  onChange,
  disabled,
}: {
  id: string;
  label: string;
  hint?: string;
  checked: boolean;
  onChange: (v: boolean) => void;
  disabled?: boolean;
}) {
  return (
    <label className="toggle" htmlFor={id}>
      <input
        id={id}
        type="checkbox"
        checked={checked}
        onChange={(e) => onChange(e.target.checked)}
        disabled={disabled}
      />
      <span className="toggle__track" aria-hidden />
      <span>
        <span className="toggle__label">{label}</span>
        {hint ? <span className="toggle__hint">{hint}</span> : null}
      </span>
    </label>
  );
}

function GeneralSection() {
  const settings = useSettings();
  const set = useSetSetting();
  const s = settings.data;
  if (!s) return null;
  return (
    <div className="stack gap-3">
      <Card title="Appearance">
        <Field
          label="Theme"
          hint="Dark is the reviewed default; light uses the same tokens but has not been visually polished yet."
        >
          <select
            className="input input--narrow"
            value={s["general.theme"]}
            onChange={(e) => set.mutate({ key: "general.theme", value: e.target.value })}
          >
            <option value="dark">Dark</option>
            <option value="light">Light</option>
          </select>
        </Field>
      </Card>
      <Card title="Review thresholds">
        <p className="muted small">
          Used by Review once prediction ships (0.3.0). High-confidence photos are eligible for
          apply; the rest are held for review.
        </p>
        <div className="row gap-4">
          <Field label="High confidence ≥" htmlFor="th-high">
            <input
              id="th-high"
              className="input input--narrow"
              type="number"
              min={0.5}
              max={1}
              step={0.05}
              value={s["review.highThreshold"]}
              onChange={(e) =>
                set.mutate({ key: "review.highThreshold", value: Number(e.target.value) })
              }
            />
          </Field>
          <Field label="Medium confidence ≥" htmlFor="th-med">
            <input
              id="th-med"
              className="input input--narrow"
              type="number"
              min={0}
              max={1}
              step={0.05}
              value={s["review.mediumThreshold"]}
              onChange={(e) =>
                set.mutate({ key: "review.mediumThreshold", value: Number(e.target.value) })
              }
            />
          </Field>
        </div>
      </Card>
    </div>
  );
}

function LightroomSection() {
  const caps = useCapabilityMatrix();
  const settings = useSettings();
  const set = useSetSetting();
  return (
    <div className="stack gap-3">
      <LightroomSetup />
      <Card title="Capability matrix">
        {caps.data?.matrix ? (
          <CapabilitySummary matrix={caps.data.matrix} live={caps.data.live} />
        ) : (
          <p className="muted">
            No capability data yet. Connect Lightroom and test the connection.
          </p>
        )}
      </Card>
      <Card title="Apply safety">
        {settings.data ? (
          <Toggle
            id="snap"
            label="Create a Develop snapshot before applying"
            hint="“Mimic Before — <timestamp>”. Kept on unless Lightroom cannot create snapshots."
            checked={settings.data["lightroom.applyCreateSnapshot"]}
            onChange={(v) => set.mutate({ key: "lightroom.applyCreateSnapshot", value: v })}
          />
        ) : null}
      </Card>
    </div>
  );
}

function PerformanceSection() {
  const settings = useSettings();
  const set = useSetSetting();
  const system = useSystemStatus();
  const s = settings.data;
  if (!s) return null;
  const engineCaps = (system.data?.engine.capabilities ?? {}) as Record<string, unknown>;
  return (
    <div className="stack gap-3">
      <Card title="Engine">
        <div className="metric-grid">
          <Metric
            label="State"
            value={system.data?.engine.state ?? "…"}
            hint={system.data?.engine.engineVersion ?? ""}
          />
          <Metric
            label="Accelerator"
            value={system.data?.engine.accelerator ?? "—"}
            hint={engineCaps["onnx"] ? "ONNX Runtime available" : "ONNX Runtime not available"}
          />
          <Metric
            label="RAW decode"
            value={engineCaps["rawDecode"] ? "LibRaw" : "unavailable"}
            hint={engineCaps["rawDecode"] ? "rawpy" : "JPEG/TIFF only"}
          />
          <Metric label="Python" value={system.data?.engine.pythonVersion ?? "—"} />
        </div>
      </Card>
      <Card title="Workers">
        <div className="row gap-4">
          <Field
            label="Worker concurrency"
            htmlFor="conc"
            hint="Reserved for parallel analysis in a later release; the engine currently processes batches sequentially."
          >
            <input
              id="conc"
              className="input input--narrow"
              type="number"
              min={1}
              max={16}
              value={s["performance.workerConcurrency"]}
              onChange={(e) =>
                set.mutate({ key: "performance.workerConcurrency", value: Number(e.target.value) })
              }
            />
          </Field>
          <Field label="Inference batch size" htmlFor="batch">
            <input
              id="batch"
              className="input input--narrow"
              type="number"
              min={1}
              max={256}
              value={s["performance.inferenceBatchSize"]}
              onChange={(e) =>
                set.mutate({ key: "performance.inferenceBatchSize", value: Number(e.target.value) })
              }
            />
          </Field>
        </div>
      </Card>
      <Card title="Visual encoder">
        <p className="muted small">
          Mimic currently uses the built-in statistical embedding (stats_v1). A downloadable ONNX
          encoder is manifest-driven and SHA-256 verified; no manifest ships in this build, so
          nothing is downloaded.
        </p>
      </Card>
    </div>
  );
}

function StorageSection() {
  const info = useAppInfo();
  const settings = useSettings();
  const set = useSetSetting();
  return (
    <div className="stack gap-3">
      <Card title="Data location">
        <div className="path-row">
          <code className="path">{info.data?.dataRoot ?? "…"}</code>
          <Button
            size="sm"
            variant="ghost"
            icon={<Copy />}
            onClick={() =>
              info.data &&
              navigator.clipboard
                .writeText(info.data.dataRoot)
                .then(() => toast.success("Path copied"))
            }
            aria-label="Copy data path"
          />
        </div>
        <p className="muted small mt-2">
          Contains the database, preview and embedding caches, models, logs and the plugin copy.
          Mimic never deletes your source photos.
        </p>
      </Card>
      <Card title="Caches">
        {settings.data ? (
          <Field
            label="Preview cache limit (MB)"
            htmlFor="cache"
            hint="Cleanup enforcing this limit is scheduled for 0.2.0; the value is stored now."
          >
            <input
              id="cache"
              className="input input--narrow"
              type="number"
              min={256}
              step={256}
              value={settings.data["performance.previewCacheMaxMb"]}
              onChange={(e) =>
                set.mutate({ key: "performance.previewCacheMaxMb", value: Number(e.target.value) })
              }
            />
          </Field>
        ) : null}
      </Card>
    </div>
  );
}

function PrivacySection() {
  const settings = useSettings();
  return (
    <div className="stack gap-3">
      <Card title="Everything stays on this computer">
        <p>
          Image analysis, metadata, training data, models and edit history are processed and stored
          locally. Mimic makes exactly one kind of network request: checking GitHub Releases for
          updates, which you can turn off under Updates.
        </p>
        {settings.data ? (
          <Toggle
            id="net"
            label="Network features"
            hint="No network features exist yet. This stays off until a future release adds an explicitly opt-in feature."
            checked={settings.data["privacy.networkFeatures"]}
            onChange={() => {}}
            disabled
          />
        ) : null}
      </Card>
    </div>
  );
}

function UpdatesSection() {
  const settings = useSettings();
  const set = useSetSetting();
  const update = useQuery({
    queryKey: qk.update,
    queryFn: ipc.updateState,
    refetchInterval: 10_000,
  });
  const updater = useUpdater(false);
  const s = settings.data;
  const u = update.data;
  return (
    <div className="stack gap-3">
      <Card
        title="Version"
        actions={
          <Button
            size="sm"
            icon={<RefreshCw />}
            onClick={() => updater.checkNow()}
            loading={updater.checking}
          >
            Check for updates
          </Button>
        }
      >
        <div className="metric-grid">
          <Metric label="Current" value={u?.currentVersion ?? "…"} />
          <Metric
            label="Latest seen"
            value={u?.latestSeenVersion ?? "—"}
            hint={`checked ${formatRelative(u?.lastCheckedAt)}`}
          />
          <Metric
            label="Staged"
            value={u?.stagedVersion ?? "—"}
            hint={u?.stagedVersion ? "installs when idle on restart" : ""}
          />
          <Metric label="Channel" value={u?.channel ?? "…"} />
        </div>
        {u?.updateError ? (
          <InlineError title="Last check failed">
            {u.updateError} — Mimic keeps working; it will retry later.
          </InlineError>
        ) : null}
        {updater.available ? (
          <div className="row gap-2 mt-3">
            <Badge tone="accent">Mimic {updater.available.version} available</Badge>
            <Button variant="primary" size="sm" onClick={() => updater.install()}>
              Install and restart
            </Button>
          </div>
        ) : null}
      </Card>
      {s ? (
        <Card title="Preferences">
          <div className="stack gap-3">
            <Toggle
              id="auto"
              label="Download updates automatically"
              hint="Updates are staged in the background and never interrupt ingest, training or a Lightroom apply."
              checked={s["updates.automatic"]}
              onChange={(v) => set.mutate({ key: "updates.automatic", value: v })}
            />
            <Field
              label="Channel"
              hint="Beta receives pre-releases. Nightly builds never auto-install into either channel."
            >
              <select
                className="input input--narrow"
                value={s["updates.channel"]}
                onChange={(e) => set.mutate({ key: "updates.channel", value: e.target.value })}
              >
                <option value="stable">Stable</option>
                <option value="beta">Beta</option>
              </select>
            </Field>
          </div>
        </Card>
      ) : null}
    </div>
  );
}

function DiagnosticsSection() {
  const qc = useQueryClient();
  const bundle = useQuery({ queryKey: qk.diagnostics, queryFn: ipc.diagnostics });
  const events = useQuery({
    queryKey: qk.events,
    queryFn: () => ipc.recentEvents(60, "warn"),
    refetchInterval: 10_000,
  });
  const settings = useSettings();
  const set = useSetSetting();
  const restart = useMutation({
    mutationFn: ipc.restartEngine,
    onSuccess: () => {
      toast.success("Engine restarted");
      qc.invalidateQueries({ queryKey: qk.system });
      qc.invalidateQueries({ queryKey: qk.diagnostics });
    },
    onError: (e: Error) => toast.danger("Engine restart failed", e.message),
  });
  const copyBundle = async () => {
    const b = await ipc.diagnostics();
    await navigator.clipboard.writeText(JSON.stringify(b, null, 2));
    toast.success("Diagnostic bundle copied", "No tokens; paths redacted unless enabled below.");
  };
  const tableCounts = useMemo(
    () => Object.fromEntries(bundle.data?.tableCounts ?? []),
    [bundle.data],
  );
  return (
    <div className="stack gap-3">
      <Card
        title="Health"
        actions={
          <>
            <Button
              size="sm"
              icon={<FolderOpen />}
              onClick={() =>
                ipc
                  .openLogsFolder()
                  .catch((e: Error) => toast.warning("Cannot open logs", e.message))
              }
            >
              Open log folder
            </Button>
            <Button size="sm" icon={<Copy />} onClick={copyBundle}>
              Copy diagnostic bundle
            </Button>
            <Button
              size="sm"
              variant="secondary"
              icon={<RefreshCw />}
              onClick={() => restart.mutate()}
              loading={restart.isPending}
            >
              Restart engine
            </Button>
          </>
        }
      >
        <div className="metric-grid">
          <Metric
            label="Database schema"
            value={`v${bundle.data?.dbSchemaVersion ?? "…"}`}
            hint={`${tableCounts["assets"] ?? 0} assets · ${tableCounts["edit_snapshots"] ?? 0} snapshots`}
          />
          <Metric
            label="Engine"
            value={(bundle.data?.engine as { state?: string } | undefined)?.state ?? "…"}
            hint={(bundle.data?.engine as { lastError?: string } | undefined)?.lastError ?? ""}
          />
          <Metric
            label="Bridge"
            value={
              (bundle.data?.bridge as { connected?: boolean } | undefined)?.connected
                ? "connected"
                : "listening"
            }
            hint={(bundle.data?.bridge as { baseUrl?: string } | undefined)?.baseUrl ?? ""}
          />
          <Metric label="Generated" value={formatDate(bundle.data?.generatedAt)} />
        </div>
        {settings.data ? (
          <div className="mt-3">
            <Toggle
              id="paths"
              label="Include full file paths in the diagnostic bundle"
              hint="Off by default; paths are reduced to file names."
              checked={settings.data["diagnostics.includePaths"]}
              onChange={(v) => set.mutate({ key: "diagnostics.includePaths", value: v })}
            />
          </div>
        ) : null}
      </Card>
      <Card title="Recent warnings and errors">
        {events.data && events.data.length > 0 ? (
          <table className="table">
            <thead>
              <tr>
                <th>When</th>
                <th>Category</th>
                <th>Event</th>
                <th>Details</th>
              </tr>
            </thead>
            <tbody>
              {events.data.map((e) => (
                <tr key={e.id}>
                  <td className="muted">{formatDate(e.createdAt)}</td>
                  <td>
                    <Badge tone={e.level === "error" ? "danger" : "warning"}>{e.category}</Badge>
                  </td>
                  <td>{e.eventType}</td>
                  <td className="mono small">{e.payloadJson.slice(0, 160)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        ) : (
          <p className="muted">No warnings or errors recorded.</p>
        )}
      </Card>
      <p className="muted small">Storage: previews {formatBytes(0)} tracked in 0.2.0.</p>
    </div>
  );
}
