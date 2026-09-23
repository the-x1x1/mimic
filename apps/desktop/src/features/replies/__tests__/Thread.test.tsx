import { render, screen } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { ReactElement } from "react";
import { describe, expect, it } from "vitest";
import type { DashboardThread, Draft } from "@mimic/contracts";
import { Thread } from "../RepliesPage";

const draft: Draft = {
  id: "d1",
  participantId: null,
  conversationId: "c1",
  channel: "email",
  situationId: null,
  incomingMessage: "can you confirm Friday?",
  incomingMessageId: "m2",
  intent: null,
  generatedText: "yeah friday works, i'll send the numbers thursday night",
  finalText: null,
  provider: "local",
  model: "llama3.1:8b",
  context: {},
  promptHash: "abc",
  evidence: {},
  createdAt: "2026-09-20T10:00:00Z",
  resolvedAt: null,
  outcome: null,
};

const thread = (over: Partial<DashboardThread> = {}): DashboardThread => ({
  conversationId: "c1",
  channel: "email",
  subject: null,
  isGroup: false,
  messageCount: 2,
  lastMessage: "can you confirm Friday?",
  lastMessageAt: "2026-09-20T09:00:00Z",
  lastMessageId: "m2",
  participant: null,
  hasRelationshipProfile: false,
  draft,
  automated: null,
  mark: null,
  ...over,
});

function setup(first: DashboardThread) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const wrap = (el: ReactElement) => (
    <QueryClientProvider client={client}>{el}</QueryClientProvider>
  );
  const view = render(wrap(<Thread thread={first} />));
  return { rerender: (next: DashboardThread) => view.rerender(wrap(<Thread thread={next} />)) };
}

describe("a card shows only the draft written for the message on it", () => {
  it("drops the draft when they write again", () => {
    const { rerender } = setup(thread());
    expect(screen.getByText(draft.generatedText)).toBeInTheDocument();
    rerender(
      thread({ lastMessageId: "m3", lastMessage: "actually, can we do Monday?", draft: null }),
    );
    expect(screen.queryByText(draft.generatedText)).toBeNull();
    expect(screen.getByText("actually, can we do Monday?")).toBeInTheDocument();
  });

  it("shows a draft prepared in the background once the screen has it", () => {
    const { rerender } = setup(thread({ draft: null }));
    expect(screen.queryByText(draft.generatedText)).toBeNull();
    rerender(thread());
    expect(screen.getByText(draft.generatedText)).toBeInTheDocument();
  });

  it("offers to take the message off the list, naming whose it is, after the message", () => {
    setup(thread());
    const control = screen.getByRole("button", { name: /doesn't need a reply/i });
    const message = screen.getByText("can you confirm Friday?");
    expect(
      message.compareDocumentPosition(control) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
  });
});
