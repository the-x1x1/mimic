import { Badge } from "@mimic/ui";
import type { EngineStatus, ProviderInfo, UpdateState } from "@mimic/contracts";

export function EngineBadge({ status }: { status: EngineStatus | undefined }) {
  if (!status) return null;
  const tone =
    status.state === "ready" ? "success" : status.state === "failed" ? "danger" : "neutral";
  const label =
    status.state === "ready"
      ? "Engine ready"
      : status.state === "failed"
        ? "Engine unavailable"
        : "Engine starting";
  return (
    <Badge tone={tone} title={status.lastError ?? undefined}>
      {label}
    </Badge>
  );
}

/**
 * Where drafts are written. This is a privacy fact, so it is in the top bar
 * rather than buried in Settings.
 */
export function ProviderBadge({
  provider,
  health,
}: {
  provider: ProviderInfo | undefined;
  health?: { reachable: boolean; error: string | null };
}) {
  if (!provider) return <Badge tone="warning">No model configured</Badge>;
  const where = provider.local ? "Local model" : `Sends to ${provider.displayName}`;
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
      <Badge tone="info">Update {state.latestSeenVersion} available</Badge>
    </button>
  );
}
