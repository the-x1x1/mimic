import { useState, type ReactNode } from "react";
import { Button, Card, Field, InlineError } from "@mimic/ui";
import type { ProviderSetup } from "@mimic/contracts";
import { useSaveSignInSetup, useSignInSetup } from "@/hooks/useSources";

type With = "microsoft" | "google";

/**
 * Signing in to a mailbox in the browser needs Mimic registered with the
 * provider (0.10.0-alpha.59). A build made with a registration has it; the
 * user can register Mimic themselves and paste what Microsoft or Google
 * shows, which is kept with their other saved sign-ins and used at once.
 * Says which is in use; never shows Google's secret back.
 */
export function SignInSetupSection() {
  const setup = useSignInSetup();
  return (
    <Card title="Signing in with Microsoft and Google" id="sign-in-setup">
      <p className="neutral">
        Outlook.com, Hotmail and Microsoft 365 mailboxes &mdash; and Gmail, if you&rsquo;d rather
        not make an app password &mdash; are signed in to in your browser, through an app registered
        with Microsoft or Google. You can register me yourself and paste what they show you here.
        It&rsquo;s kept with your other saved sign-ins, sealed to your Windows account, and used
        from the next sign-in on. A mailbox already signed in with that provider under another
        registration then needs <strong>Sign in again</strong>, under Your mail.
      </p>
      {setup.isError ? <InlineError>{(setup.error as Error).message}</InlineError> : null}
      {setup.data ? (
        <>
          <ProviderPart
            with="microsoft"
            name="Microsoft"
            setup={setup.data.microsoft}
            steps={
              <ol className="small">
                <li>At entra.microsoft.com, open App registrations, then New registration.</li>
                <li>
                  Name it Mimic. Under who can use it, choose accounts in any organizational
                  directory and personal Microsoft accounts.
                </li>
                <li>
                  For the redirect URI, choose Public client/native (mobile &amp; desktop) and enter
                  http://localhost
                </li>
                <li>
                  Register, then copy the Application (client) ID from its Overview and paste it
                  below.
                </li>
              </ol>
            }
            note="You needn't add API permissions: I ask to read and send your mail when you sign in, and you approve it there. A work or school account's admin may have to approve it first."
          />
          <ProviderPart
            with="google"
            name="Google"
            setup={setup.data.google}
            steps={
              <ol className="small">
                <li>
                  At console.cloud.google.com, make a project, and under APIs &amp; Services enable
                  the Gmail API: its scope is listed only then.
                </li>
                <li>
                  Under Google Auth Platform: in Branding, name it Mimic with your email; in
                  Audience, choose Internal when every account you&rsquo;ll connect is in your own
                  Google Workspace &mdash; no warning, and sign-ins don&rsquo;t lapse &mdash; or
                  else External and publish it; in Data Access, add the scope
                  https://mail.google.com/
                </li>
                <li>In Clients, create a client of the type Desktop app.</li>
                <li>Copy its client ID and client secret, and paste both below.</li>
                <li>
                  For Google Chat (a Google Workspace account), in the same project: enable the
                  Google Chat API; on its Configuration page give the app a name, an avatar address
                  and a description, with interactive features off; and in Data Access add openid,
                  email, profile and the Chat scopes chat.spaces.readonly, chat.messages.readonly,
                  chat.messages.create and chat.memberships.readonly. A project in your Workspace
                  can choose Internal as its audience instead: no warning, no lapsing.
                </li>
              </ol>
            }
            note="Published without Google's verification, Google warns you it hasn't verified the app when you sign in: that's your own registration. Left in testing instead, Google asks you to sign in again every seven days. For Gmail, an app password is simpler."
          />
        </>
      ) : null}
    </Card>
  );
}

function ProviderPart({
  with: provider,
  name,
  setup,
  steps,
  note,
}: {
  with: With;
  name: string;
  setup: ProviderSetup;
  steps: ReactNode;
  note: string;
}) {
  const { save, forget } = useSaveSignInSetup();
  const [clientId, setClientId] = useState("");
  const [secret, setSecret] = useState("");
  const [open, setOpen] = useState(false);
  const google = provider === "google";
  const busy = save.isPending || forget.isPending;
  const error = (save.error ?? forget.error) as Error | null;

  let state: string;
  if (setup.own) state = `I sign in with ${name} as your own registration, ${setup.clientId}.`;
  else if (setup.builtIn) state = `I sign in with ${name} as this copy's own registration.`;
  else state = `Not set up: this copy of me was built without a registration with ${name}.`;

  return (
    <section className="stack gap-2" aria-label={`Signing in with ${name}`}>
      <h3 className="small">{name}</h3>
      <p>{state}</p>
      {setup.own ? (
        <div className="row gap-2">
          <Button size="sm" variant="ghost" disabled={busy} onClick={() => forget.mutate(provider)}>
            {setup.builtIn ? "Use this copy's own instead" : "Forget it"}
          </Button>
        </div>
      ) : null}
      <div>
        <button
          type="button"
          className="linkish"
          aria-expanded={open}
          onClick={() => setOpen((v) => !v)}
        >
          {setup.own ? `Use another registration with ${name}` : `Register me with ${name}`}
        </button>
      </div>
      {open ? (
        <>
          {steps}
          <p className="muted small">{note}</p>
          <form
            className="stack gap-2"
            onSubmit={(e) => {
              e.preventDefault();
              save.mutate(
                { with: provider, clientId, clientSecret: google ? secret : null },
                {
                  onSuccess: () => {
                    setClientId("");
                    setSecret("");
                    setOpen(false);
                  },
                },
              );
            }}
          >
            <Field
              label={google ? "Client ID" : "Application (client) ID"}
              htmlFor={`${provider}-client`}
            >
              <input
                id={`${provider}-client`}
                value={clientId}
                autoComplete="off"
                spellCheck={false}
                onChange={(e) => setClientId(e.target.value)}
              />
            </Field>
            {google ? (
              <Field label="Client secret" htmlFor="google-secret">
                <input
                  id="google-secret"
                  type="password"
                  value={secret}
                  autoComplete="off"
                  onChange={(e) => setSecret(e.target.value)}
                />
              </Field>
            ) : null}
            <div className="row gap-2">
              <Button
                type="submit"
                size="sm"
                disabled={busy || !clientId.trim() || (google && !secret.trim())}
              >
                Keep it
              </Button>
            </div>
          </form>
        </>
      ) : null}
      {error ? <InlineError>{error.message}</InlineError> : null}
    </section>
  );
}
