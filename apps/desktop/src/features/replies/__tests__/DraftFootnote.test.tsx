import { render, screen } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Draft, ReplyEnvelope } from "@mimic/contracts";
import { DraftReview } from "../RepliesPage";

// The real hooks run; only the native calls are replaced.
const native = vi.hoisted(() => ({
  replyEnvelope: vi.fn(),
  autoReply: vi.fn(),
  whyNotByItself: vi.fn(),
}));
vi.mock("@/lib/ipc", () => ({ ipc: native }));

const NOT_EMAIL =
  "This reply isn't to an email, so it can't be sent from here. Use this copies it.";

const draft: Draft = {
  id: "d1",
  participantId: "p1",
  conversationId: "c1",
  channel: "sms",
  situationId: null,
  incomingMessage: "did you see the game last night",
  incomingMessageId: "m5",
  intent: "yes, caught the second half",
  generatedText: "yes caught the second half crazy ending",
  finalText: null,
  provider: "local",
  model: "llama3.2:3b",
  context: {},
  promptHash: "abc",
  evidence: {},
  createdAt: "2026-11-08T10:00:00Z",
  resolvedAt: null,
  outcome: null,
  alternativeTo: null,
};

function renderReview() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <DraftReview draft={draft} onDone={vi.fn()} />
    </QueryClientProvider>,
  );
}

describe("what the line under a draft says", () => {
  beforeEach(() => {
    for (const f of Object.values(native)) f.mockReset();
    native.autoReply.mockResolvedValue(null);
    native.whyNotByItself.mockResolvedValue(null);
  });

  it("says why it can't be sent once, without saying Use this copies it twice", async () => {
    native.replyEnvelope.mockResolvedValue({
      draftId: "d1",
      mailboxes: [],
      mailbox: null,
      to: [],
      otherAddresses: [],
      othersOnIt: [],
      subject: null,
      whyNot: NOT_EMAIL,
      held: null,
    } as unknown as ReplyEnvelope);
    renderReview();
    const line = await screen.findByText(/can't be sent from here/);
    expect(line.textContent).toBe(`Written from what you told me you wanted to say. ${NOT_EMAIL}`);
    expect(line.textContent?.match(/copies it/g)).toHaveLength(1);
  });
});
