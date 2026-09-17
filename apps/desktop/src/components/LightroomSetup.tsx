import { Button, Card, InlineError } from "@mimic/ui";
import { Copy, FolderOpen, RefreshCw } from "lucide-react";
import { openPath } from "@tauri-apps/plugin-opener";
import {
  useInstallPlugin,
  useLightroomStatus,
  usePluginSetup,
  useTestLightroom,
} from "@/hooks/useLightroom";
import { toast } from "@/state/toast";
import { isTauri } from "@/lib/tauri";

export function LightroomSetup({ compact = false }: { compact?: boolean }) {
  const status = useLightroomStatus();
  const setup = usePluginSetup();
  const install = useInstallPlugin();
  const test = useTestLightroom();
  const connected = status.data?.bridge.connected ?? false;
  const conn = status.data?.bridge.connection;

  const copyPath = async () => {
    if (!setup.data) return;
    try {
      await navigator.clipboard.writeText(setup.data.pluginPath);
      toast.success("Path copied");
    } catch {
      toast.warning("Clipboard unavailable", setup.data.pluginPath);
    }
  };

  return (
    <Card
      title="Lightroom Classic"
      actions={
        <Button
          size="sm"
          onClick={() => test.mutate()}
          loading={test.isPending}
          icon={<RefreshCw />}
        >
          Test connection
        </Button>
      }
    >
      {connected && conn ? (
        <div className="stack gap-2">
          <div className="row gap-2 wrap">
            <span className="dot dot--good" /> Connected to{" "}
            <strong>{conn.catalogName ?? "catalog"}</strong> · Lightroom {conn.lightroomVersion} ·
            plugin {conn.pluginVersion}
          </div>
          {!conn.capabilities.probeHadPhoto ? (
            <p className="muted small">
              Select any photo in Lightroom and test again so Mimic can probe which develop settings
              this version exposes.
            </p>
          ) : null}
          {!conn.capabilities.canApply ? (
            <InlineError title="Apply is disabled on this connection">
              Lightroom did not grant catalog write access or preset application. Reading edits
              still works.
            </InlineError>
          ) : null}
        </div>
      ) : (
        <div className="stack gap-3">
          <p className="muted">
            Open Lightroom Classic and make sure the Mimic plugin is enabled. Mimic does not open or
            modify your catalog file — the plugin talks to this app over a local loopback
            connection.
          </p>
          {status.data && !status.data.pluginInstalled ? (
            <div className="row gap-2">
              <Button
                variant="primary"
                onClick={() => install.mutate()}
                loading={install.isPending}
                disabled={!status.data.pluginSourceAvailable}
              >
                Prepare plugin folder
              </Button>
              {!status.data.pluginSourceAvailable ? (
                <span className="muted small">Plugin files are missing from this build.</span>
              ) : null}
            </div>
          ) : null}
          {setup.data ? (
            <ol className="steps">
              {setup.data.steps.map((s) => (
                <li key={s}>{s}</li>
              ))}
            </ol>
          ) : null}
          {setup.data ? (
            <div className="path-row">
              <code className="path">{setup.data.pluginPath}</code>
              <Button
                size="sm"
                variant="ghost"
                icon={<Copy />}
                onClick={copyPath}
                aria-label="Copy plugin path"
              />
              <Button
                size="sm"
                variant="ghost"
                icon={<FolderOpen />}
                onClick={() =>
                  isTauri() &&
                  openPath(setup.data!.pluginPath).catch((e) =>
                    toast.warning("Cannot open folder", String(e)),
                  )
                }
                aria-label="Reveal plugin folder"
              />
            </div>
          ) : null}
          {status.data?.lastKnown && !compact ? (
            <p className="muted small">
              Last connected:{" "}
              {status.data.lastKnown.lightroomVersion
                ? `Lightroom ${status.data.lastKnown.lightroomVersion}`
                : "unknown version"}{" "}
              · {new Date(status.data.lastKnown.lastSeenAt).toLocaleString()}
            </p>
          ) : null}
        </div>
      )}
    </Card>
  );
}
