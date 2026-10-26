import { useState } from "react";
import { Button } from "@mimic/ui";
import { ipc } from "@/lib/ipc";
import { describeProbe } from "./describeProbe";

/** One part tried, and what came of it. */
interface Tried {
  what: string;
  ok: boolean;
  said: string;
}

/**
 * Try every part of replying for you now, sending nothing (0.10.0-alpha.51):
 * for each mailbox, signing in to read (folders looked at, no message
 * opened) and to send (signed in, nothing sent); each Slack workspace and
 * Teams account, as a check reads it (0.10.0-alpha.69); then whether the
 * model that writes replies answers. One after another, so a slow server
 * doesn't hide which part failed.
 */
export function CheckEverything({
  mailboxes,
  chats = [],
  ready,
}: {
  mailboxes: { id: string; name: string }[];
  /** Slack workspaces and Teams accounts. */
  chats?: { id: string; name: string; connector: "slack" | "teams" }[];
  /** Whether the mailboxes are known yet: until then, a check would leave them out. */
  ready: boolean;
}) {
  const [tried, setTried] = useState<Tried[] | null>(null);
  const [running, setRunning] = useState(false);

  async function run() {
    setRunning(true);
    const out: Tried[] = [];
    const note = (t: Tried) => {
      out.push(t);
      setTried([...out]);
    };
    setTried([]);
    for (const m of mailboxes) {
      try {
        note({
          what: `Reading ${m.name}`,
          ok: true,
          said: describeProbe(await ipc.testMailboxReading(m.id)),
        });
      } catch (e) {
        note({ what: `Reading ${m.name}`, ok: false, said: (e as Error).message });
      }
      try {
        const host = await ipc.testMailboxSending(m.id);
        note({
          what: `Sending from ${m.name}`,
          ok: true,
          said: `Signed in to ${host}. Nothing was sent.`,
        });
      } catch (e) {
        note({ what: `Sending from ${m.name}`, ok: false, said: (e as Error).message });
      }
    }
    for (const c of chats) {
      try {
        const said =
          c.connector === "slack" ? await ipc.testSlack(c.id) : await ipc.testTeams(c.id);
        note({ what: `Reading ${c.name}`, ok: true, said });
      } catch (e) {
        note({ what: `Reading ${c.name}`, ok: false, said: (e as Error).message });
      }
    }
    try {
      const providers = await ipc.providerState();
      const active = providers.providers.find((p) => p.id === providers.active);
      if (!active) {
        note({ what: "Writing replies", ok: false, said: "No model is chosen to write replies." });
      } else {
        try {
          await ipc.checkProvider(active.id);
          note({ what: "Writing replies", ok: true, said: `${active.displayName} answers.` });
        } catch (e) {
          note({
            what: "Writing replies",
            ok: false,
            said: `${active.displayName}: ${(e as Error).message}`,
          });
        }
      }
    } catch (e) {
      note({ what: "Writing replies", ok: false, said: (e as Error).message });
    }
    setRunning(false);
  }

  return (
    <div className="stack gap-1" role="group" aria-label="Check it all now">
      <div className="row gap-2">
        <Button size="sm" disabled={running || !ready} onClick={() => void run()}>
          {running ? "Checking…" : "Check it all now (sends nothing)"}
        </Button>
      </div>
      {tried && tried.length > 0 ? (
        <ul className="stack gap-1" aria-label="What the check found">
          {tried.map((t) => (
            <li key={t.what} className="row gap-2">
              <span aria-hidden="true">{t.ok ? "✓" : "✕"}</span>
              <span>
                <strong>{t.what}:</strong> {t.said}
              </span>
            </li>
          ))}
        </ul>
      ) : null}
    </div>
  );
}
