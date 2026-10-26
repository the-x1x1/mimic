import { render, screen, waitFor, within } from "@testing-library/react";
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
});
