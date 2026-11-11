import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { DashboardThread, Draft } from "@mimic/contracts";
import { Thread } from "../RepliesPage";

// The real hooks run; only the native calls are replaced.
const native = vi.hoisted(() => ({
  generateDraft: vi.fn(),
  situations: vi.fn(),
  resolveDraft: vi.fn(),
}));
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

function renderIt(t: DashboardThread, oneOfSeveral = false) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <Thread thread={t} oneOfSeveral={oneOfSeveral} />
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

const prepared: Draft = {
  id: "d1",
  participantId: "p1",
  conversationId: "c1",
  channel: "sms",
  situationId: null,
  incomingMessage: "did you see the game last night",
  intent: null,
  generatedText: "yes caught the second half crazy ending",
  finalText: null,
  provider: "local",
  model: "llama3.1:8b",
  context: {},
  promptHash: "abc",
  evidence: {},
  createdAt: "2026-11-08T02:00:00Z",
  resolvedAt: null,
  outcome: null,
  incomingMessageId: "m5",
  alternativeTo: null,
};

describe("what the card says once its draft is gone", () => {
  beforeEach(() => {
    for (const f of Object.values(native)) f.mockReset();
    native.situations.mockResolvedValue([]);
    native.resolveDraft.mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText: vi.fn().mockResolvedValue(undefined) },
    });
  });

  it("says a draft was turned down, not that none was written, and takes the keyboard", async () => {
    renderIt(thread({ draft: prepared }));
    fireEvent.click(screen.getByRole("button", { name: "Not this one" }));
    const form = await screen.findByRole("form", { name: "Write a reply to Dad" });
    expect(within(form).getByText("Not that one, then")).toBeInTheDocument();
    expect(within(form).getByText(/what to say instead/)).toBeInTheDocument();
    expect(within(form).queryByText(/haven\u2019t written this one yet/)).toBeNull();
    expect(within(form).queryByText(/don't know yet how you write/)).toBeNull();
    expect(
      within(form).getByRole("textbox", { name: "Roughly what you want to say to Dad" }),
    ).toHaveFocus();
  });

  it("says a taken draft is theirs to send, and leaves the keyboard alone", async () => {
    renderIt(thread({ draft: prepared }));
    fireEvent.click(screen.getByRole("button", { name: "Use this" }));
    const form = await screen.findByRole("form", { name: "Write a reply to Dad" });
    expect(within(form).getByText("That one\u2019s yours to send")).toBeInTheDocument();
    expect(
      within(form).getByRole("textbox", { name: "Roughly what you want to say to Dad" }),
    ).not.toHaveFocus();
  });

  it("still says nothing was written when nothing was", () => {
    renderIt(thread());
    expect(screen.getByText("I haven\u2019t written this one yet")).toBeInTheDocument();
  });

  it("opens the editor with the caret after the last word", () => {
    renderIt(thread({ draft: prepared }));
    fireEvent.click(screen.getByRole("button", { name: "Change it" }));
    const box = screen.getByDisplayValue(prepared.generatedText) as HTMLTextAreaElement;
    fireEvent.focus(box);
    expect(box.selectionStart).toBe(prepared.generatedText.length);
  });
});

describe("two cards from one person", () => {
  beforeEach(() => {
    for (const f of Object.values(native)) f.mockReset();
    native.situations.mockResolvedValue([]);
  });

  it("says which conversation a card with no subject is", () => {
    renderIt(thread(), true);
    expect(screen.getByText("Text messages · 5 messages")).toBeInTheDocument();
  });

  it("says nothing more for the only card from someone, or one with a subject", () => {
    renderIt(thread());
    expect(screen.queryByText(/5 messages/)).toBeNull();
    renderIt(thread({ subject: "Saturday", channel: "email" }), true);
    expect(screen.getByText("Saturday")).toBeInTheDocument();
    expect(screen.queryByText(/Email · 5 messages/)).toBeNull();
  });
});
