import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Draft, OutboxEntry, ReplyEnvelope } from "@mimic/contracts";
import { DraftReview } from "../RepliesPage";

// The real hooks run; only the native calls are replaced.
const native = vi.hoisted(() => ({
  replyEnvelope: vi.fn(),
  stopHeldReply: vi.fn(),
  sendHeldReplyNow: vi.fn(),
  resolveDraft: vi.fn(),
  writeAnotherDraft: vi.fn(),
  addDraftPreference: vi.fn(),
  autoReply: vi.fn(),
  setAutoReply: vi.fn(),
  whyNotByItself: vi.fn(),
}));
vi.mock("@/lib/ipc", () => ({ ipc: native }));

const draft: Draft = {
  id: "d1",
  participantId: "p1",
  conversationId: "c1",
  channel: "email",
  situationId: null,
  incomingMessage: "the place by the river?",
  intent: null,
  generatedText: "yes, the river place. 12:30?",
  finalText: null,
  provider: "local",
  model: "llama3.1:8b",
  context: {},
  promptHash: "abc",
  evidence: {},
  createdAt: "2026-10-26T09:00:00Z",
  resolvedAt: null,
  outcome: null,
  incomingMessageId: "m3",
  alternativeTo: null,
};

const held: OutboxEntry = {
  id: "o1",
  draftId: "d1",
  sourceId: "s1",
  conversationId: "c1",
  answersMessageId: "m3",
  participantId: "p1",
  fromAddress: "c@example.com",
  to: ["ada@example.com"],
  cc: [],
  subject: "Re: Lunch",
  body: "yes, the river place. 12:30?",
  messageId: "x@example.com",
  how: "autonomous",
  status: "held",
  sendAfter: new Date(Date.now() + 9 * 60_000).toISOString(),
  error: null,
  createdAt: "2026-10-26T09:00:00Z",
  updatedAt: "2026-10-26T09:00:00Z",
  sentAt: null,
};

const envelope: ReplyEnvelope = {
  draftId: "d1",
  mailboxes: [
    {
      sourceId: "s1",
      name: "c@example.com",
      fromAddress: "c@example.com",
      server: "smtp.gmail.com",
      whyNot: null,
    },
  ],
  mailbox: "s1",
  to: ["ada@example.com"],
  otherAddresses: [],
  othersOnIt: [],
  subject: "Re: Lunch",
  whyNot:
    "I'm holding a reply to this to send by myself. Stop it first if you'd rather send your own.",
  held,
};

function renderReview() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <DraftReview draft={draft} onDone={vi.fn()} />
    </QueryClientProvider>,
  );
}

const rules = {
  enabled: true,
  mailboxes: ["s1"],
  who: "listed" as const,
  people: ["p1", "p2"],
  never: [],
  holdMinutes: 10,
  dailyLimit: 10,
  enabledAt: "2026-10-26T09:00:00.000Z",
};

describe("a reply held to go by itself", () => {
  beforeEach(() => {
    for (const f of Object.values(native)) f.mockReset();
    native.replyEnvelope.mockResolvedValue(envelope);
    native.autoReply.mockResolvedValue({ rules, running: true, lastDay: 1 });
    native.whyNotByItself.mockResolvedValue(null);
    native.setAutoReply.mockImplementation(async (r: typeof rules) => ({
      rules: r,
      running: true,
      lastDay: 1,
    }));
  });

  it("says when it goes and to whom, and offers no Send of its own beside it", async () => {
    renderReview();
    const box = await screen.findByRole("group", { name: "Held to send by itself" });
    expect(
      within(box).getByText(/I'll send this to ada@example.com by myself in 9 minutes/),
    ).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Send" })).toBeNull();
    expect(screen.getByText(/Stop it first if you'd rather send your own/)).toBeInTheDocument();
  });

  it("says it is a rehearsal while rehearsing, and offers no Send it now", async () => {
    native.autoReply.mockResolvedValue({
      rules: { ...rules, rehearse: true },
      running: true,
      lastDay: 1,
    });
    renderReview();
    const box = await screen.findByRole("group", { name: "Held to send by itself" });
    expect(
      await within(box).findByText(
        /^Rehearsing: I'd send this to ada@example.com by myself in 9 minutes/,
      ),
    ).toBeInTheDocument();
    expect(within(box).queryByRole("button", { name: "Send it now" })).toBeNull();
    expect(within(box).getByRole("button", { name: "Stop it" })).toBeInTheDocument();
  });

  it("can be stopped, or sent now", async () => {
    native.stopHeldReply.mockResolvedValue({ ...held, status: "canceled" });
    native.sendHeldReplyNow.mockResolvedValue({ ...held, status: "sent" });
    renderReview();
    const box = await screen.findByRole("group", { name: "Held to send by itself" });
    fireEvent.click(within(box).getByRole("button", { name: "Stop it" }));
    await waitFor(() => expect(native.stopHeldReply).toHaveBeenCalledWith("o1"));
    fireEvent.click(within(box).getByRole("button", { name: "Send it now" }));
    await waitFor(() => expect(native.sendHeldReplyNow).toHaveBeenCalledWith("o1"));
  });

  it("can be stopped for good for its person", async () => {
    native.stopHeldReply.mockResolvedValue({ ...held, status: "canceled" });
    renderReview();
    const box = await screen.findByRole("group", { name: "Held to send by itself" });
    fireEvent.click(
      await within(box).findByRole("button", { name: "Stop it, and never by myself to them" }),
    );
    await waitFor(() => expect(native.stopHeldReply).toHaveBeenCalledWith("o1"));
    await waitFor(() =>
      expect(native.setAutoReply).toHaveBeenCalledWith({
        ...rules,
        people: ["p2"],
        never: ["p1"],
      }),
    );
  });
});
