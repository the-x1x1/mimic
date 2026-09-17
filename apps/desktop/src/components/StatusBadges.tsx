import { Badge } from "@mimic/ui";
import { Cable, Cpu, Download } from "lucide-react";
import type { BridgeStatus, EngineStatus, UpdateState } from "@mimic/contracts";

export function ConnectionBadge({ status }: { status: BridgeStatus | undefined }) {
  if (!status) return <Badge>Lightroom · …</Badge>;
  if (status.connected && status.connection) {
    return (
      <Badge
        tone="success"
        title={`Lightroom ${status.connection.lightroomVersion} · ${status.connection.catalogName ?? "catalog"}`}
      >
        <Cable size={12} /> Lightroom connected
      </Badge>
    );
  }
  return (
    <Badge tone="neutral" title="Open Lightroom Classic with the Mimic plugin enabled">
      <Cable size={12} /> Lightroom offline
    </Badge>
  );
}

export function EngineBadge({ status }: { status: EngineStatus | undefined }) {
  if (!status) return <Badge>Engine · …</Badge>;
  const tone =
    status.state === "ready" ? "success" : status.state === "starting" ? "info" : "danger";
  const label =
    status.state === "ready"
      ? `Engine ready${status.accelerator && status.accelerator !== "cpu" ? ` · ${status.accelerator}` : ""}`
      : status.state === "starting"
        ? "Engine starting"
        : "Engine unavailable";
  return (
    <Badge tone={tone} title={status.lastError ?? status.engineVersion ?? ""}>
      <Cpu size={12} /> {label}
    </Badge>
  );
}

export function UpdateBadge({
  state,
  onClick,
}: {
  state: UpdateState | undefined;
  onClick?: () => void;
}) {
  if (
    !state?.stagedVersion &&
    !(state?.latestSeenVersion && state.lastUpdateResult === "available")
  )
    return null;
  const v = state.stagedVersion ?? state.latestSeenVersion;
  return (
    <button className="badge-button" onClick={onClick} title="Open update settings">
      <Badge tone="accent">
        <Download size={12} /> {state.stagedVersion ? `Update ${v} ready` : `Update ${v} available`}
      </Badge>
    </button>
  );
}
