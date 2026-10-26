import type { ReactNode } from "react";
import { useQuery } from "@tanstack/react-query";
import { Link } from "react-router-dom";
import { Button, Card } from "@mimic/ui";
import { hourOf } from "@mimic/contracts";
import { useSettings } from "@/hooks/useSystem";
import { useSources } from "@/hooks/useSources";
import { useAutoReply, useMailboxSending } from "@/hooks/useSending";
import { ipc } from "@/lib/ipc";
import { qk } from "@/app/queryClient";
import { CheckEverything } from "./CheckEverything";

/** Bring a card on this page into view: links within a page can't be hash links here. */
function goTo(id: string) {
  document.getElementById(id)?.scrollIntoView?.({ behavior: "smooth", block: "start" });
}

function Step({
  done,
  children,
  action,
}: {
  done: boolean | null;
  children: ReactNode;
  action?: ReactNode;
}) {
  return (
    <li className="row gap-2">
      <span aria-hidden="true">{done === null ? "·" : done ? "✓" : "○"}</span>
      <span>
        {children} {action}
      </span>
    </li>
  );
}

/** One mailbox: can a reply be sent from it? */
function MailboxCanSend({ sourceId, name }: { sourceId: string; name: string }) {
  const sending = useMailboxSending(sourceId);
  const can = sending.data ? sending.data.sending !== null : null;
  const signed = Boolean(sending.data?.signature);
  return (
    <Step
      done={can}
      action={
        can === false ? (
          <Link to="/sources" className="muted small">
            Set up sending under Your mail
          </Link>
        ) : null
      }
    >
      {can === false
        ? `Replies can't be sent from ${name} yet.`
        : signed
          ? `Replies can be sent from ${name}, with your signature.`
          : `Replies can be sent from ${name}.`}
    </Step>
  );
}

/**
 * Everything replying for you needs, in one place, in order: a mailbox, a
 * way to send from it, replies prepared in advance, replying by myself if you
 * want it, and Mimic running when its window is closed. Each says where it
 * stands and where to change it. Nothing here changes anything itself.
 */
export function ReplyingForYou() {
  const settings = useSettings();
  const sources = useSources();
  const autoReply = useAutoReply();
  const atSignIn = useQuery({ queryKey: qk.startWithWindows, queryFn: ipc.startWithWindows });
  const mailboxes = (sources.data ?? []).filter((s) => s.connector === "imap");
  const prepared = settings.data?.["assist.autoDraft"];
  const keepRunning = settings.data?.["app.keepRunning"];
  const notify = settings.data?.["notify.enabled"];
  const byItself = autoReply.data?.rules.enabled;
  const hours = autoReply.data?.rules.hours;
  const never = autoReply.data?.rules.never.length ?? 0;
  const wait = settings.data?.["send.undoSeconds"];

  return (
    <Card title="Replying for you">
      <p className="neutral">
        Where each part of replying for you stands. Everything is off or waits for you until you
        turn it on.
      </p>
      <ul className="stack gap-1" aria-label="Replying for you">
        <Step
          done={sources.data ? mailboxes.length > 0 : null}
          action={
            mailboxes.length === 0 ? (
              <Link to="/sources" className="muted small">
                Connect one under Your mail
              </Link>
            ) : null
          }
        >
          {mailboxes.length === 0
            ? "No mailbox is connected, so there's nothing to reply to or from."
            : mailboxes.length === 1
              ? "A mailbox is connected."
              : `${mailboxes.length} mailboxes are connected.`}
        </Step>
        {mailboxes.map((m) => (
          <MailboxCanSend key={m.id} sourceId={m.id} name={m.name} />
        ))}
        <Step
          done={prepared ?? null}
          action={
            prepared === false ? (
              <Button size="sm" variant="ghost" onClick={() => goTo("prepare")}>
                Turn it on below
              </Button>
            ) : null
          }
        >
          {prepared === false
            ? "I write replies only when you ask."
            : "I write replies for new mail as it comes, for you to look over."}
        </Step>
        <Step
          done={byItself ?? null}
          action={
            <Button size="sm" variant="ghost" onClick={() => goTo("by-myself")}>
              {byItself ? "Change it below" : "Turn it on below, if you want it"}
            </Button>
          }
        >
          {byItself
            ? `I send some by myself, after holding each for ${autoReply.data?.rules.holdMinutes ?? 10} minutes${hours ? `, only between ${hourOf(hours.from)} and ${hourOf(hours.to)}` : ""}${never > 0 ? `, never to the ${never === 1 ? "one person" : `${never} people`} you named` : ""}.`
            : "Every reply waits for you to send it."}
        </Step>
        {typeof wait === "number" ? (
          <Step
            done={wait > 0}
            action={
              <Button size="sm" variant="ghost" onClick={() => goTo("mail")}>
                Change it below
              </Button>
            }
          >
            {wait > 0
              ? `When you press Send it, I wait ${wait} seconds, so you can take it back.`
              : "Send it sends at once, with no moment to take it back."}
          </Step>
        ) : null}
        <Step
          done={keepRunning ?? null}
          action={
            <Button size="sm" variant="ghost" onClick={() => goTo("background")}>
              Change it below
            </Button>
          }
        >
          {keepRunning === false
            ? "Closing my window quits me, and nothing is checked or sent until you open me."
            : atSignIn.data
              ? "I keep running when my window is closed, and start when you sign in to Windows."
              : "I keep running when my window is closed, but don't start when you sign in to Windows."}
        </Step>
        <Step done={notify ?? null}>
          {notify === false
            ? "I don't tell you with notifications; the home screen is the only place I say what's waiting."
            : "I tell you with a notification when replies are ready or something went."}
        </Step>
      </ul>
      <CheckEverything
        mailboxes={mailboxes.map((m) => ({ id: m.id, name: m.name }))}
        ready={sources.data !== undefined}
      />
    </Card>
  );
}
