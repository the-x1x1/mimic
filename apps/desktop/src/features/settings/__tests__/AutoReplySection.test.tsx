import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { AutoReplyRules, AutoReplyState } from "@mimic/contracts";
import { AutoReplySection } from "../AutoReplySection";

// The real hooks run; only the native calls are replaced.
const native = vi.hoisted(() => ({
  autoReply: vi.fn(),
  setAutoReply: vi.fn(),
  sources: vi.fn(),
  people: vi.fn(),
}));
vi.mock("@/lib/ipc", () => ({ ipc: native }));

const off: AutoReplyRules = {
  enabled: false,
  mailboxes: [],
  who: "known",
  people: [],
  never: [],
  holdMinutes: 10,
  dailyLimit: 10,
  enabledAt: null,
};

const mailbox = {
  id: "s1",
  connector: "imap",
  name: "c@example.com",
  channel: "email",
  location: null,
  config: {},
  status: "imported",
  createdAt: "2026-10-01T00:00:00Z",
  lastImportedAt: null,
  messageCount: 10,
  lastError: null,
};

function renderIt(
  rules: AutoReplyRules,
  running = false,
  people: unknown[] = [],
  sources: unknown[] = [mailbox],
) {
  native.autoReply.mockResolvedValue({ rules, running, lastDay: 0 } satisfies AutoReplyState);
  native.setAutoReply.mockImplementation(async (r: AutoReplyRules) => ({
    rules: r,
    running: r.enabled,
    lastDay: 0,
  }));
  native.sources.mockResolvedValue(sources);
  native.people.mockResolvedValue({
    people,
    peopleTotal: people.length,
    automatedSendersTotal: 0,
    automatedSenders: [],
    showingAutomated: false,
  });
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <AutoReplySection />
    </QueryClientProvider>,
  );
}

describe("replying by myself", () => {
  beforeEach(() => {
    for (const f of Object.values(native)) f.mockReset();
  });

  it("offers a Slack workspace beside the mailboxes, for direct messages only", async () => {
    renderIt(
      off,
      false,
      [],
      [
        mailbox,
        { ...mailbox, id: "s2", connector: "slack", name: "Acme (Slack)", channel: "chat" },
      ],
    );
    expect(await screen.findByText("Through these mailboxes and workspaces")).toBeInTheDocument();
    expect(screen.getByRole("checkbox", { name: "Acme (Slack)" })).toBeInTheDocument();
    expect(screen.getByText(/In Slack I answer a direct message only/)).toBeInTheDocument();
  });

  it("is off until turned on, and turning it on says what it does first", async () => {
    renderIt(off);
    expect(await screen.findByText("Off. Every reply waits for you.")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Reply by myself…" }));
    expect(native.setAutoReply).not.toHaveBeenCalled();
    const confirm = screen.getByRole("group", { name: "Turn on replying by myself" });
    expect(within(confirm).getByText(/without asking each time/)).toBeInTheDocument();
    // Everything it checks, said before it is on.
    expect(screen.getByText(/that I read the same day/)).toBeInTheDocument();
    expect(
      screen.getByText(/a gap left to fill\s+in, a link they didn.t send/),
    ).toBeInTheDocument();
    expect(screen.getByText(/You can rehearse it first/)).toBeInTheDocument();
    expect(screen.getByText(/from me on your other computer, too/)).toBeInTheDocument();
    fireEvent.click(within(confirm).getByRole("button", { name: "Turn it on" }));
    await waitFor(() =>
      expect(native.setAutoReply).toHaveBeenCalledWith({ ...off, enabled: true, rehearse: false }),
    );
  });

  it("can be rehearsed first, and says so", async () => {
    // A mailbox already ticked, so what it would do can be said.
    const ticked = { ...off, mailboxes: ["s1"] };
    renderIt(ticked);
    fireEvent.click(await screen.findByRole("button", { name: "Reply by myself…" }));
    const confirm = screen.getByRole("group", { name: "Turn on replying by myself" });
    expect(within(confirm).getByText(/Or rehearse first/)).toBeInTheDocument();
    fireEvent.click(within(confirm).getByRole("button", { name: "Rehearse first" }));
    await waitFor(() =>
      expect(native.setAutoReply).toHaveBeenCalledWith({
        ...ticked,
        enabled: true,
        rehearse: true,
      }),
    );
    expect(await screen.findByText(/^Rehearsing: I'd answer/)).toBeInTheDocument();
    const rehearse = screen.getByRole("checkbox", { name: /^Rehearse:/ });
    expect(rehearse).toBeChecked();
    expect(
      screen.getByText(/the replies I held while rehearsing wait for you/),
    ).toBeInTheDocument();
    fireEvent.click(rehearse);
    await waitFor(() =>
      expect(native.setAutoReply).toHaveBeenLastCalledWith(
        expect.objectContaining({ enabled: true, rehearse: false }),
      ),
    );
  });

  it("answers through the mailboxes ticked, and turns off in one step", async () => {
    renderIt({ ...off, enabled: true }, true);
    fireEvent.click(await screen.findByRole("checkbox", { name: "c@example.com" }));
    await waitFor(() =>
      expect(native.setAutoReply).toHaveBeenCalledWith({
        ...off,
        enabled: true,
        mailboxes: ["s1"],
      }),
    );
    fireEvent.click(screen.getByRole("button", { name: "Turn it off and stop anything waiting" }));
    await waitFor(() =>
      expect(native.setAutoReply).toHaveBeenLastCalledWith(
        expect.objectContaining({ enabled: false }),
      ),
    );
  });

  it("leaves out by name whomever the user would rather answer", async () => {
    renderIt({ ...off, enabled: true, mailboxes: ["s1"] }, true, [
      { participant: { id: "p-ada", displayName: "Ada Lovelace" } },
      { participant: { id: "p-bob", displayName: "Bob" } },
    ]);
    const group = await screen.findByRole("group", { name: "Never by myself to" });
    expect(within(group).getByText("No one yet.")).toBeInTheDocument();
    fireEvent.change(within(group).getByRole("searchbox", { name: "Find someone to leave out" }), {
      target: { value: "ada" },
    });
    fireEvent.click(await within(group).findByRole("checkbox", { name: "Ada Lovelace" }));
    await waitFor(() =>
      expect(native.setAutoReply).toHaveBeenCalledWith({
        ...off,
        enabled: true,
        mailboxes: ["s1"],
        never: ["p-ada"],
      }),
    );
    expect(
      await screen.findByText(/people you've written to yourself, except the one you named/),
    ).toBeInTheDocument();
  });

  it("takes someone off the never list when they are chosen again, and lists anyone named", async () => {
    renderIt(
      {
        ...off,
        enabled: true,
        mailboxes: ["s1"],
        who: "listed",
        people: [],
        never: ["p-ada", "p-far"],
      },
      true,
      [{ participant: { id: "p-ada", displayName: "Ada Lovelace" } }],
    );
    const group = await screen.findByRole("group", { name: "Never by myself to" });
    expect(await within(group).findByRole("checkbox", { name: "Ada Lovelace" })).toBeChecked();
    expect(within(group).getByRole("checkbox", { name: "Someone else you named" })).toBeChecked();
    const chosen = screen
      .getAllByRole("checkbox", { name: "Ada Lovelace" })
      .find((b) => !group.contains(b));
    expect(chosen).not.toBeChecked();
    fireEvent.click(chosen as HTMLElement);
    await waitFor(() =>
      expect(native.setAutoReply).toHaveBeenLastCalledWith(
        expect.objectContaining({ people: ["p-ada"], never: ["p-far"] }),
      ),
    );
  });

  it("sends by itself only at the hours chosen", async () => {
    renderIt({ ...off, enabled: true, mailboxes: ["s1"] }, true);
    fireEvent.click(await screen.findByRole("checkbox", { name: /Only at the hours I choose/ }));
    await waitFor(() =>
      expect(native.setAutoReply).toHaveBeenLastCalledWith(
        expect.objectContaining({ hours: { from: 8, to: 20 } }),
      ),
    );
    expect(await screen.findByText(/only between 8:00 and 20:00/)).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("Until"), { target: { value: "18" } });
    await waitFor(() =>
      expect(native.setAutoReply).toHaveBeenLastCalledWith(
        expect.objectContaining({ hours: { from: 8, to: 18 } }),
      ),
    );
  });
});
