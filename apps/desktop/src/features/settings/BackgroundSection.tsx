import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Card, InlineError } from "@mimic/ui";
import { useSettings } from "@/hooks/useSystem";
import { ipc } from "@/lib/ipc";
import { qk } from "@/app/queryClient";

/**
 * Whether Mimic keeps running when its window is closed, starts with
 * Windows, and says so with a notification when there is something to look
 * at. Checking mail, reading it as it arrives and replying by myself only
 * happen while Mimic runs, so this card says that plainly.
 */
export function BackgroundSection() {
  const qc = useQueryClient();
  const settings = useSettings();
  const keepRunning = settings.data?.["app.keepRunning"] ?? true;
  const notify = settings.data?.["notify.enabled"] ?? true;
  const atSignIn = useQuery({ queryKey: qk.startWithWindows, queryFn: ipc.startWithWindows });
  const setAtSignIn = useMutation({
    mutationFn: (on: boolean) => ipc.setStartWithWindows(on),
    onSuccess: (now) => qc.setQueryData(qk.startWithWindows, now),
  });

  return (
    <Card title="When the window is closed" id="background">
      <p className="neutral">
        I only check your mail, write replies and send them while I&rsquo;m running. With the first
        box ticked, closing my window leaves me in the notification area, by the clock; click my
        icon to open me, or right-click it to quit.
      </p>
      <label className="row gap-2">
        <input
          type="checkbox"
          checked={keepRunning}
          onChange={(e) => void ipc.setSetting("app.keepRunning", e.target.checked)}
        />
        <span>Keep running in the notification area when I close the window</span>
      </label>
      <label className="row gap-2">
        <input
          type="checkbox"
          checked={atSignIn.data ?? false}
          disabled={atSignIn.data === undefined || setAtSignIn.isPending}
          onChange={(e) => setAtSignIn.mutate(e.target.checked)}
        />
        <span>Start when I sign in to Windows, with no window, only the icon</span>
      </label>
      <label className="row gap-2">
        <input
          type="checkbox"
          checked={notify}
          onChange={(e) => void ipc.setSetting("notify.enabled", e.target.checked)}
        />
        <span>Tell me with a Windows notification when replies are ready or something went</span>
      </label>
      <p className="muted small">
        A notification says how many and where to look &mdash; never who wrote or what they said,
        since Windows keeps notifications where anyone at this computer can read them. None comes
        while you&rsquo;re looking at me.
      </p>
      {atSignIn.error ? <InlineError>{(atSignIn.error as Error).message}</InlineError> : null}
      {setAtSignIn.error ? <InlineError>{(setAtSignIn.error as Error).message}</InlineError> : null}
    </Card>
  );
}
