import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { BridgeEvent, EngineEvent, EngineStatus, JobEvent } from "@mimic/contracts";
import { isTauri } from "./tauri";

type Handler<T> = (payload: T) => void;

async function on<T>(
  name: string,
  parse: (raw: unknown) => T | null,
  handler: Handler<T>,
): Promise<UnlistenFn> {
  if (!isTauri()) return () => {};
  return listen(name, (ev) => {
    const parsed = parse(ev.payload);
    if (parsed) handler(parsed);
  });
}

export const events = {
  onJob: (h: Handler<JobEvent>) => on("jobs://event", (r) => JobEvent.safeParse(r).data ?? null, h),
  onEngineEvent: (h: Handler<EngineEvent>) =>
    on("engine://event", (r) => EngineEvent.safeParse(r).data ?? null, h),
  onEngineStatus: (h: Handler<EngineStatus>) =>
    on("engine://status", (r) => EngineStatus.safeParse(r).data ?? null, h),
  onLightroom: (h: Handler<BridgeEvent>) =>
    on("lightroom://event", (r) => BridgeEvent.safeParse(r).data ?? null, h),
  onReady: (h: Handler<unknown>) => on("app://ready", (r) => r, h),
};
