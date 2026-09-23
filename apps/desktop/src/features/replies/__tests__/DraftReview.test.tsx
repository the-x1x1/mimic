import { render, screen } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { describe, expect, it } from "vitest";
import type { Draft } from "@mimic/contracts";
import { DraftReview } from "../RepliesPage";

const draft = (over: Partial<Draft> = {}): Draft => ({
  id: "d1",
  participantId: "p1",
  conversationId: "c1",
  channel: "email",
  situationId: null,
  incomingMessage: "can you confirm Friday?",
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
  incomingMessageId: "m2",
  ...over,
});

function renderReview(d: Draft) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <DraftReview draft={d} onDone={() => undefined} />
    </QueryClientProvider>,
  );
}

describe("reviewing a prepared draft", () => {
  it("offers approve, modify and reject, and never claims to send", () => {
    renderReview(draft());
    expect(screen.getByText("Use this")).toBeInTheDocument();
    expect(screen.getByText("Change it")).toBeInTheDocument();
    expect(screen.getByText("Not this one")).toBeInTheDocument();
    expect(screen.getByText(/I never send anything/)).toBeInTheDocument();
    expect(screen.queryByText(/^Send$/)).toBeNull();
  });

  it("warns that a draft prepared in advance had no intent behind it", () => {
    renderReview(draft());
    expect(screen.getByText(/didn't tell me what you wanted to say/)).toBeInTheDocument();
  });

  it("does not warn when the user said what they wanted", () => {
    renderReview(draft({ intent: "confirm friday, promise numbers thursday" }));
    expect(screen.queryByText(/didn't tell me what you wanted to say/)).toBeNull();
    expect(screen.getByText(/Written from what you told me/)).toBeInTheDocument();
  });

  it("shows the draft text itself, so approving is not blind", () => {
    renderReview(draft());
    expect(screen.getByText(/i'll send the numbers thursday night/)).toBeInTheDocument();
  });
});
