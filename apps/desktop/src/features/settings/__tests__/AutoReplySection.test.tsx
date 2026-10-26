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

function renderIt(rules: AutoReplyRules, running = false) {
  native.autoReply.mockResolvedValue({ rules, running, lastDay: 0 } satisfies AutoReplyState);
  native.setAutoReply.mockImplementation(async (r: AutoReplyRules) => ({
    rules: r,
    running: r.enabled,
    lastDay: 0,
  }));
  native.sources.mockResolvedValue([mailbox]);
  native.people.mockResolvedValue({
    people: [],
    peopleTotal: 0,
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

  it("is off until turned on, and turning it on says what it does first", async () => {
    renderIt(off);
    expect(await screen.findByText("Off. Every reply waits for you.")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Reply by myself…" }));
    expect(native.setAutoReply).not.toHaveBeenCalled();
    const confirm = screen.getByRole("group", { name: "Turn on replying by myself" });
    expect(within(confirm).getByText(/without asking each time/)).toBeInTheDocument();
    fireEvent.click(within(confirm).getByRole("button", { name: "Turn it on" }));
    await waitFor(() =>
      expect(native.setAutoReply).toHaveBeenCalledWith({ ...off, enabled: true }),
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
});
