import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type {
  ConversationPage,
  PersonConversation,
  PersonConversations as Page,
} from "@mimic/contracts";
import { CONVERSATION_GONE, PersonConversations } from "../PersonConversations";

// The real hooks and query client run; only the native calls are replaced.
const native = vi.hoisted(() => ({
  personConversations: vi.fn(),
  conversationEnd: vi.fn(),
  conversationPage: vi.fn(),
  markThread: vi.fn(),
}));
vi.mock("@/lib/ipc", () => ({ ipc: native }));

const convo = (id: string, over: Partial<PersonConversation> = {}): PersonConversation => ({
  conversationId: id,
  subject: `About ${id}`,
  channel: "email",
  isGroup: false,
  messageCount: 3,
  lastMessageAt: "2026-09-02T10:00:00Z",
  decidingMessageId: `${id}-last`,
  standing: "answered",
  mark: null,
  ...over,
});

const page = (conversations: PersonConversation[], more = 0): Page => ({
  conversations,
  more,
  waitingWithinDays: 30,
});

const message = (id: string, body: string, direction: "self" | "other" = "other") => ({
  id,
  direction,
  author: direction === "self" ? null : "Ada Lovelace",
  sentAt: "2026-09-01T10:00:00Z",
  body,
  automated: null,
  filing: null,
});

function renderIt() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <PersonConversations participantId="ada" name="Ada" onClose={() => {}} />
    </QueryClientProvider>,
  );
}

describe("every conversation with someone, not only what is waiting", () => {
  beforeEach(() => {
    for (const f of Object.values(native)) f.mockReset();
    native.markThread.mockResolvedValue(true);
  });

  it("says where each stands, and puts one left off the list back on it", async () => {
    native.personConversations.mockResolvedValue(
      page([
        convo("lunch", { standing: "waiting" }),
        convo("receipt", { standing: "automated" }),
        convo("old", { standing: "quiet" }),
        convo("done", { standing: "answered" }),
      ]),
    );
    renderIt();
    expect(await screen.findByText("About lunch")).toBeInTheDocument();
    expect(native.personConversations).toHaveBeenCalledWith("ada", null, 20);
    expect(screen.getByText(/On your list: waiting on you\./)).toBeInTheDocument();
    expect(screen.getByText(/its last message looks automated/)).toBeInTheDocument();
    expect(
      screen.getByText(/the message it waits on is more than 30 days old/),
    ).toBeInTheDocument();
    expect(screen.getByText(/You wrote last\./)).toBeInTheDocument();
    // Only the two left off the list can be put on it.
    expect(screen.getAllByRole("button", { name: /^Put it on my list/ })).toHaveLength(2);
    fireEvent.click(screen.getByRole("button", { name: "Put it on my list: About receipt" }));
    await waitFor(() =>
      expect(native.markThread).toHaveBeenCalledWith("receipt", "receipt-last", "needs_reply"),
    );
  });

  it("reads a conversation from its end, and further back when asked, the earliest on top", async () => {
    native.personConversations.mockResolvedValue(page([convo("lunch")]));
    const end: ConversationPage = {
      messages: [message("m3", "see you then", "self"), message("m4", "great")],
      more: 2,
    };
    const earlier: ConversationPage = {
      messages: [message("m1", "lunch on friday?"), message("m2", "which place?", "self")],
      more: 0,
    };
    native.conversationEnd.mockResolvedValue(end);
    native.conversationPage.mockResolvedValue(earlier);
    renderIt();
    fireEvent.click(await screen.findByRole("button", { name: "Read it: About lunch" }));
    expect(await screen.findByText("great")).toBeInTheDocument();
    expect(native.conversationEnd).toHaveBeenCalledWith("lunch", 20);
    fireEvent.click(screen.getByRole("button", { name: "Show 2 earlier messages" }));
    expect(await screen.findByText("lunch on friday?")).toBeInTheDocument();
    expect(native.conversationPage).toHaveBeenCalledWith("lunch", "m3", "earlier", 20);
    const items = Array.from(document.querySelectorAll(".thread__history-item")).map(
      (li) => li.textContent ?? "",
    );
    expect(items).toHaveLength(4);
    ["lunch on friday?", "which place?", "see you then", "great"].forEach((body, i) =>
      expect(items[i]).toContain(body),
    );
    expect(screen.queryByRole("button", { name: /earlier message/ })).toBeNull();
  });

  it("reads on to older conversations from the last one shown", async () => {
    native.personConversations
      .mockResolvedValueOnce(page([convo("lunch", { lastMessageAt: "2026-09-05T10:00:00Z" })], 1))
      .mockResolvedValueOnce(page([convo("older", { lastMessageAt: null })]));
    renderIt();
    fireEvent.click(await screen.findByRole("button", { name: "Show 1 more conversation" }));
    expect(await screen.findByText("About older")).toBeInTheDocument();
    expect(native.personConversations).toHaveBeenLastCalledWith(
      "ada",
      { at: "2026-09-05T10:00:00Z", id: "lunch" },
      20,
    );
  });

  it("says so when a conversation was deleted while it was open", async () => {
    native.personConversations.mockResolvedValue(page([convo("lunch")]));
    native.conversationEnd.mockRejectedValue(
      Object.assign(new Error("conversation lunch"), { code: "not_found" }),
    );
    renderIt();
    fireEvent.click(await screen.findByRole("button", { name: "Read it: About lunch" }));
    expect(await screen.findByText(CONVERSATION_GONE)).toBeInTheDocument();
  });

  it("says when there is nothing to show", async () => {
    native.personConversations.mockResolvedValue(page([]));
    renderIt();
    expect(
      await screen.findByText("I haven’t read a conversation with Ada in it."),
    ).toBeInTheDocument();
  });
});
