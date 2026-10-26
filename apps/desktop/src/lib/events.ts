import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { AutoReplyRun, EngineEvent, EngineStatus, JobEvent } from "@mimic/contracts";
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
  onReady: (h: Handler<unknown>) => on("app://ready", (r) => r, h),
  /**
   * The answer-from-anywhere shortcut was pressed: what the clipboard held,
   * possibly nothing. It goes to Write and nowhere else.
   */
  onPasted: (h: Handler<{ text: string }>) =>
    on(
      "write://pasted",
      (r) => {
        const text = (r as { text?: unknown } | null)?.text;
        return typeof text === "string" ? { text } : null;
      },
      h,
    ),
  /**
   * Replies sent by themselves were held, sent, stopped or failed — or their
   * rules changed (then every count is absent, read as 0).
   */
  onSending: (h: Handler<AutoReplyRun>) =>
    on(
      "sending://changed",
      (r) => AutoReplyRun.safeParse(r).data ?? { held: 0, sent: 0, stopped: 0, failed: 0 },
      h,
    ),
};
