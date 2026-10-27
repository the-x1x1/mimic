import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { MemoryRouter } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ReplyingForYou } from "../ReplyingForYou";

// The real hooks run; only the native calls are replaced.
const native = vi.hoisted(() => ({
  settings: vi.fn(),
  sources: vi.fn(),
  autoReply: vi.fn(),
  mailboxSending: vi.fn(),
  startWithWindows: vi.fn(),
  testMailboxReading: vi.fn(),
  testMailboxSending: vi.fn(),
  testSlack: vi.fn(),
  testTeams: vi.fn(),
  testTelegram: vi.fn(),
  providerState: vi.fn(),
  checkProvider: vi.fn(),
}));
vi.mock("@/lib/ipc", () => ({ ipc: native }));

const mailbox = (id: string, name: string) => ({
  id,
  connector: "imap",
  name,
  channel: "email",
  location: null,
  config: {},
  status: "ready",
  createdAt: "2026-10-01T00:00:00Z",
  lastImportedAt: null,
  messageCount: 10,
  lastError: null,
});

const rules = (enabled: boolean) => ({
  rules: {
    enabled,
    mailboxes: [],
    who: "known",
    people: [],
    never: [],
    holdMinutes: 10,
    dailyLimit: 10,
    enabledAt: null,
  },
  running: enabled,
  lastDay: 0,
});

function renderIt() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <MemoryRouter>
        <ReplyingForYou />
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

describe("replying for you, set up in one place", () => {
  beforeEach(() => {
    for (const f of Object.values(native)) f.mockReset();
    native.startWithWindows.mockResolvedValue(false);
  });

  it("starts from a mailbox, and says where to connect one", async () => {
    native.settings.mockResolvedValue({
      "assist.autoDraft": false,
      "app.keepRunning": true,
      "notify.enabled": true,
    });
    native.sources.mockResolvedValue([]);
    native.autoReply.mockResolvedValue(rules(false));
    renderIt();
    expect(
      await screen.findByText(/No mailbox is connected, so there's nothing to reply to or from/),
    ).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Connect one under Your mail" })).toHaveAttribute(
      "href",
      "/sources",
    );
    expect(await screen.findByText("I write replies only when you ask.")).toBeInTheDocument();
    expect(await screen.findByText("Every reply waits for you to send it.")).toBeInTheDocument();
  });

  it("lists Slack and Teams, and checks them with the rest, sending nothing", async () => {
    native.settings.mockResolvedValue({ "assist.autoDraft": true });
    native.sources.mockResolvedValue([
      { ...mailbox("s7", "Acme (Slack)"), connector: "slack", channel: "chat" },
      {
        ...mailbox("s8", "c@formicaria.example (Teams)"),
        connector: "teams",
        channel: "chat",
        status: "failed",
        lastError: { message: "Microsoft wants you to sign in to Teams again." },
      },
      { ...mailbox("s9", "C Haynes (Telegram)"), connector: "telegram_account", channel: "chat" },
    ]);
    native.autoReply.mockResolvedValue(rules(false));
    native.testSlack.mockResolvedValue("Read your direct messages in Acme as c. Nothing was sent.");
    native.testTeams.mockRejectedValue(new Error("Microsoft wants you to sign in to Teams again."));
    native.testTelegram.mockResolvedValue("Signed in to Telegram as C Haynes. Nothing was sent.");
    native.providerState.mockResolvedValue({ providers: [], active: null });
    renderIt();
    const list = await screen.findByRole("list", { name: "Replying for you" });
    expect(await within(list).findByText("No mailbox is connected.")).toBeInTheDocument();
    expect(
      within(list).getByText("Acme (Slack) is connected: replies to it go in as you, from here."),
    ).toBeInTheDocument();
    expect(
      within(list).getByText(
        "c@formicaria.example (Teams) isn't working: Microsoft wants you to sign in to Teams again.",
      ),
    ).toBeInTheDocument();
    const check = await screen.findByRole("button", { name: "Check it all now (sends nothing)" });
    await waitFor(() => expect(check).toBeEnabled());
    fireEvent.click(check);
    const found = await screen.findByRole("list", { name: "What the check found" });
    expect(await within(found).findByText(/Read your direct messages in Acme/)).toBeInTheDocument();
    expect(await within(found).findByText(/sign in to Teams again/)).toBeInTheDocument();
    expect(native.testSlack).toHaveBeenCalledWith("s7");
    expect(native.testTeams).toHaveBeenCalledWith("s8");
    expect(await within(found).findByText(/Signed in to Telegram as C Haynes/)).toBeInTheDocument();
    expect(native.testTelegram).toHaveBeenCalledWith("s9");
    expect(
      within(list).getByText(
        "C Haynes (Telegram) is connected: replies to it go in as you, from here.",
      ),
    ).toBeInTheDocument();
  });

  it("says which mailboxes can send, and what runs by itself", async () => {
    native.settings.mockResolvedValue({
      "assist.autoDraft": true,
      "app.keepRunning": true,
      "notify.enabled": false,
      "send.undoSeconds": 5,
    });
    native.sources.mockResolvedValue([
      mailbox("s1", "c@gmail.com"),
      mailbox("s2", "c@formicaria.us"),
    ]);
    native.autoReply.mockResolvedValue(rules(true));
    native.startWithWindows.mockResolvedValue(true);
    native.mailboxSending.mockImplementation(async (id: string) => ({
      sending:
        id === "s1"
          ? {
              host: "smtp.gmail.com",
              port: 465,
              security: "tls",
              username: "c@gmail.com",
              fromAddress: "c@gmail.com",
            }
          : null,
      setByUser: false,
      keepsSent: id === "s1",
    }));
    renderIt();
    const list = await screen.findByRole("list", { name: "Replying for you" });
    expect(await within(list).findByText("2 mailboxes are connected.")).toBeInTheDocument();
    expect(
      await within(list).findByText("Replies can be sent from c@gmail.com."),
    ).toBeInTheDocument();
    expect(
      await within(list).findByText("Replies can't be sent from c@formicaria.us yet."),
    ).toBeInTheDocument();
    expect(
      await within(list).findByText("I send some by myself, after holding each for 10 minutes."),
    ).toBeInTheDocument();
    await waitFor(() =>
      expect(
        within(list).getByText(
          "I keep running when my window is closed, and start when you sign in to Windows.",
        ),
      ).toBeInTheDocument(),
    );
    expect(within(list).getByText(/I don't tell you with notifications/)).toBeInTheDocument();
    expect(
      within(list).getByText("When you press Send it, I wait 5 seconds, so you can take it back."),
    ).toBeInTheDocument();
  });

  it("says the hours, the people left out and the signatures too", async () => {
    native.settings.mockResolvedValue({ "assist.autoDraft": true, "send.undoSeconds": 0 });
    native.sources.mockResolvedValue([mailbox("s1", "c@gmail.com")]);
    const on = rules(true);
    native.autoReply.mockResolvedValue({
      ...on,
      rules: { ...on.rules, hours: { from: 8, to: 20 }, never: ["p1", "p2"] },
    });
    native.mailboxSending.mockResolvedValue({
      sending: {
        host: "smtp.gmail.com",
        port: 465,
        security: "tls",
        username: "c@gmail.com",
        fromAddress: "c@gmail.com",
      },
      setByUser: false,
      keepsSent: true,
      signature: "C Haynes",
    });
    renderIt();
    const list = await screen.findByRole("list", { name: "Replying for you" });
    expect(
      await within(list).findByText(
        "I send some by myself, after holding each for 10 minutes, only between 8:00 and 20:00, never to the 2 people you named.",
      ),
    ).toBeInTheDocument();
    expect(
      await within(list).findByText("Replies can be sent from c@gmail.com, with your signature."),
    ).toBeInTheDocument();
    expect(
      within(list).getByText("Send it sends at once, with no moment to take it back."),
    ).toBeInTheDocument();
  });

  it("checks it all now, one part after another, sending nothing", async () => {
    native.settings.mockResolvedValue({
      "assist.autoDraft": true,
      "app.keepRunning": true,
      "notify.enabled": true,
    });
    native.sources.mockResolvedValue([mailbox("s1", "c@gmail.com")]);
    native.autoReply.mockResolvedValue(rules(false));
    native.mailboxSending.mockResolvedValue({
      sending: null,
      setByUser: false,
      keepsSent: true,
      signature: null,
    });
    native.testMailboxReading.mockResolvedValue({
      folders: ["INBOX", "[Gmail]/Sent Mail"],
      sentFolder: "[Gmail]/Sent Mail",
      counts: [
        ["INBOX", 1234],
        ["[Gmail]/Sent Mail", 56],
      ],
      warnings: [],
    });
    native.testMailboxSending.mockRejectedValue(new Error("The server turned down the sign-in."));
    native.providerState.mockResolvedValue({
      providers: [{ id: "local", displayName: "Local model", local: true }],
      active: "local",
      configuredSecrets: [],
    });
    native.checkProvider.mockResolvedValue(undefined);
    renderIt();
    const check = await screen.findByRole("button", { name: "Check it all now (sends nothing)" });
    // Not before the mailboxes are known, which a check would leave out.
    await waitFor(() => expect(check).toBeEnabled());
    fireEvent.click(check);
    const found = await screen.findByRole("list", { name: "What the check found" });
    await waitFor(() => expect(within(found).getAllByRole("listitem")).toHaveLength(3));
    // Counts as this computer writes them.
    expect(
      within(found).getByText(
        `Signed in. I'd read INBOX (${(1234).toLocaleString()}), [Gmail]/Sent Mail (56).`,
      ),
    ).toBeInTheDocument();
    expect(within(found).getByText(/The server turned down the sign-in/)).toBeInTheDocument();
    expect(within(found).getByText("Local model answers.")).toBeInTheDocument();
    expect(native.testMailboxReading).toHaveBeenCalledWith("s1");
    expect(native.testMailboxSending).toHaveBeenCalledWith("s1");
    expect(native.checkProvider).toHaveBeenCalledWith("local");
  });
});
