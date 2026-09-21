import { Badge } from "@mimic/ui";
import type { EngineStatus, ProviderInfo, UpdateState } from "@mimic/contracts";

export function EngineBadge({ status }: { status: EngineStatus | undefined }) {
  if (!status) return null;
  const tone =
    status.state === "ready" ? "success" : status.state === "failed" ? "danger" : "neutral";
  // "Engine" is what the code calls the Python sidecar. On screen it is the
  // part that reads and measures, and a person only needs to know whether it
  // is working.
  const label =
    status.state === "ready"
      ? "Ready"
      : status.state === "failed"
        ? "Something isn't running"
        : "Starting up";
  return (
    <Badge tone={tone} title={status.lastError ?? undefined}>
      {label}
    </Badge>
  );
}

/**
 * Where the writing happens. This is a privacy fact before it is a technical
 * one, so it is in the top bar rather than buried in Settings, and it says
 * where the words are made rather than naming a "provider".
 */
export function ProviderBadge({
  provider,
  health,
}: {
  provider: ProviderInfo | undefined;
  health?: { reachable: boolean; error: string | null };
}) {
  if (!provider) return <Badge tone="warning">Nothing set up to write with</Badge>;
  const where = provider.local ? "Writing on this computer" : `Writing at ${provider.displayName}`;
  if (health && !health.reachable) {
    return (
      <Badge tone="danger" title={health.error ?? undefined}>
        {where} · not answering
      </Badge>
    );
  }
  return (
    <Badge tone={provider.local ? "success" : "warning"} title={provider.description}>
      {where}
    </Badge>
  );
}

export function UpdateBadge({
  state,
  onClick,
}: {
  state: UpdateState | undefined;
  onClick: () => void;
}) {
  if (!state?.latestSeenVersion || state.latestSeenVersion === state.currentVersion) return null;
  return (
    <button type="button" className="badge-button" onClick={onClick}>
      <Badge tone="info">Version {state.latestSeenVersion} is ready to install</Badge>
    </button>
  );
}
