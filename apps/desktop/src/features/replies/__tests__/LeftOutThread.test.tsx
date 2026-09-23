import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Dashboard, DashboardThread } from "@mimic/contracts";
import { LeftOutList, LeftOutThread } from "../RepliesPage";

// The real hook runs; only the native call is replaced.
const native = vi.hoisted(() => ({ markThread: vi.fn() }));
vi.mock("@/lib/ipc", () => ({ ipc: native }));

const left = (over: Partial<DashboardThread> = {}): DashboardThread => ({
  conversationId: "c1",
  channel: "email",
  subject: "The lease",
  isGroup: false,
  messageCount: 1,
  lastMessage: "any news on the lease?",
  lastMessageAt: "2026-06-01T09:00:00Z",
  lastMessageId: "m1",
  participant: null,
  hasRelationshipProfile: false,
  draft: null,
  automated: null,
  mark: null,
  quiet: true,
  ...over,
});

function renderIt(thread: DashboardThread) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <LeftOutThread thread={thread} withinDays={30} />
    </QueryClientProvider>,
  );
}

describe("a thread that has gone quiet can be put back", () => {
  beforeEach(() => {
    native.markThread.mockReset();
    native.markThread.mockResolvedValue(true);
  });

  it("says why it was left out, as a reading of its date", () => {
    renderIt(left());
    expect(
      screen.getByText(
        "Its last message is more than 30 days old, so I've taken it that nobody is still waiting on a reply.",
      ),
    ).toBeInTheDocument();
  });

  it("puts it back by saying it needs a reply, since its age would keep it out", async () => {
    renderIt(left());
    fireEvent.click(screen.getByRole("button", { name: /it needs a reply/i }));
    await waitFor(() => expect(native.markThread).toHaveBeenCalledWith("c1", "m1", "needs_reply"));
  });

  it("gives the headers first when it also looks automated", () => {
    renderIt(left({ automated: "newsletter" }));
    expect(screen.getByText(/It looks automated to me/)).toBeInTheDocument();
    expect(screen.queryByText(/days old/)).toBeNull();
  });

  it("puts back one the user took off, old as it is, by saying it needs a reply", async () => {
    renderIt(left({ mark: "no_reply_needed" }));
    expect(screen.getByText("You said this one doesn't need a reply.")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /put it back/i }));
    await waitFor(() => expect(native.markThread).toHaveBeenCalledWith("c1", "m1", "needs_reply"));
  });
});

describe("what was left out is listed by why", () => {
  it("groups the threads by reason, in order, and says when a group is cut short", () => {
    const threads = [
      left({ conversationId: "a", lastMessageId: "a1", mark: "no_reply_needed", quiet: false }),
      left({ conversationId: "b", lastMessageId: "b1", automated: "newsletter", quiet: false }),
      left({ conversationId: "c", lastMessageId: "c1", automated: "bulk", quiet: false }),
      left({ conversationId: "d", lastMessageId: "d1" }),
    ];
    // Only the fields the list reads; the rest of the screen is not under test.
    const data = {
      leftOutThreads: threads,
      waitingWithinDays: 30,
      leftOut: { notNeeded: 1, automated: 40, quiet: 1200 },
    } as unknown as Dashboard;
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(
      <QueryClientProvider client={client}>
        <LeftOutList data={data} />
      </QueryClientProvider>,
    );
    const headings = screen.getAllByRole("heading", { level: 3 }).map((h) => h.textContent);
    expect(headings).toEqual([
      "These look automated to me",
      "Their last message is more than 30 days old",
      "You said this one doesn't need a reply",
    ]);
    expect(screen.getByText("Here are the 2 most recent of 40.")).toBeInTheDocument();
    expect(
      screen.getByText(`Here's the most recent of ${(1200).toLocaleString()}.`),
    ).toBeInTheDocument();
    expect(screen.queryByText(/most recent of 1\./)).toBeNull();
  });
});
