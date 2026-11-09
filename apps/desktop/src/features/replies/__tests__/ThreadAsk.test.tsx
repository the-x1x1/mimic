import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { DashboardThread } from "@mimic/contracts";
import { Thread } from "../RepliesPage";

// The real hooks run; only the native calls are replaced.
const native = vi.hoisted(() => ({ generateDraft: vi.fn(), situations: vi.fn() }));
vi.mock("@/lib/ipc", () => ({ ipc: native }));

const thread = (over: Partial<DashboardThread> = {}): DashboardThread => ({
  conversationId: "c1",
  channel: "sms",
  subject: null,
  isGroup: false,
  messageCount: 5,
  lastMessage: "did you see the game last night",
  lastMessageAt: "2026-11-08T01:40:00Z",
  lastMessageId: "m5",
  earlier: 0,
  later: 0,
  participant: {
    id: "p1",
    displayName: "Dad",
    isSelf: false,
    relationship: null,
    notes: null,
    identifiers: [],
    createdAt: "2026-11-01T00:00:00Z",
    updatedAt: "2026-11-01T00:00:00Z",
  },
  hasRelationshipProfile: false,
  draft: null,
  alternatives: [],
  automated: null,
  mark: null,
  quiet: false,
  ...over,
});

function renderIt(t: DashboardThread) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <Thread thread={t} />
    </QueryClientProvider>,
  );
}

describe("asking for a reply in a few words", () => {
  beforeEach(() => {
    for (const f of Object.values(native)) f.mockReset();
    native.situations.mockResolvedValue([]);
    native.generateDraft.mockReturnValue(new Promise(() => {}));
  });

  it("puts the words and Write it on one line, and submitting the form writes it once", async () => {
    renderIt(thread());
    const form = screen.getByRole("form", { name: "Write a reply to Dad" });
    const box = within(form).getByRole("textbox", { name: "Roughly what you want to say to Dad" });
    expect(box.closest(".thread__ask")).toBe(
      within(form).getByRole("button", { name: "Write it" }).closest(".thread__ask"),
    );
    fireEvent.change(box, { target: { value: "yes, caught the second half" } });
    fireEvent.submit(form);
    await waitFor(() => expect(native.generateDraft).toHaveBeenCalled());
    expect(native.generateDraft.mock.calls[0]?.[0]).toMatchObject({
      conversationId: "c1",
      incomingMessageId: "m5",
      intent: "yes, caught the second half",
    });
    // Asked once, however often Enter is pressed while it writes.
    fireEvent.submit(form);
    expect(native.generateDraft).toHaveBeenCalledTimes(1);
    expect(within(form).getByRole("button", { name: "Writing…" })).toBeDisabled();
  });

  it("says in one sentence why it asks, naming who", () => {
    renderIt(thread());
    expect(
      screen.getByText(
        "I don't know yet how you write to Dad, so tell me roughly what to say and I'll put it in your words.",
      ),
    ).toBeInTheDocument();
  });

  it("asks nothing of someone it knows, beyond what to say if anything", () => {
    renderIt(thread({ hasRelationshipProfile: true }));
    expect(
      screen.getByText(
        "Tell me roughly what to say, or leave it empty and I'll answer what they asked.",
      ),
    ).toBeInTheDocument();
  });
});
