import { fireEvent, render, screen } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ConversationPage, ThreadMessage, Toward } from "@mimic/contracts";
import { GONE, RestOfThread } from "../RepliesPage";

// The real query runs; only the native call is replaced.
const native = vi.hoisted(() => ({ conversationPage: vi.fn() }));
vi.mock("@/lib/ipc", () => ({ ipc: native }));

const message = (
  over: Partial<ThreadMessage> & Pick<ThreadMessage, "id" | "body">,
): ThreadMessage => ({
  direction: "other",
  author: "Ada Lovelace",
  sentAt: "2026-09-19T08:00:00Z",
  automated: null,
  ...over,
});

// The page just before the message on screen, and the one before that.
const nearest: ConversationPage = {
  messages: [
    message({ id: "m3", body: "sure, which day?", direction: "self", author: null }),
    message({ id: "m4", body: "Friday or Monday" }),
  ],
  more: 2,
};
const furthest: ConversationPage = {
  messages: [
    message({ id: "m1", body: "can we meet?" }),
    message({ id: "m2", body: "I'm away until Tuesday", automated: "auto_reply" }),
  ],
  more: 0,
};

function renderIt(toward: Toward, count: number) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <RestOfThread conversationId="c1" messageId="m5" toward={toward} count={count} />
    </QueryClientProvider>,
  );
}

const bodies = (container: HTMLElement) =>
  Array.from(container.querySelectorAll("p.letter")).map((p) => p.textContent);

describe("the rest of the conversation a waiting message is part of", () => {
  beforeEach(() => {
    native.conversationPage.mockReset();
  });

  it("reads nothing until asked, then what came before, oldest first, a page at a time", async () => {
    native.conversationPage.mockResolvedValueOnce(nearest).mockResolvedValueOnce(furthest);
    const { container } = renderIt("earlier", 4);
    expect(native.conversationPage).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "Show the 4 messages before this one" }));
    expect(await screen.findByText("sure, which day?")).toBeInTheDocument();
    expect(native.conversationPage).toHaveBeenCalledWith("c1", "m5", "earlier", 20);
    expect(screen.getByText("You wrote")).toBeInTheDocument();
    expect(screen.getByText("Ada wrote")).toBeInTheDocument();

    // Further back is read on from the oldest message shown, and goes on top.
    fireEvent.click(screen.getByRole("button", { name: "Show 2 earlier messages" }));
    expect(await screen.findByText("can we meet?")).toBeInTheDocument();
    expect(native.conversationPage).toHaveBeenLastCalledWith("c1", "m3", "earlier", 20);
    expect(bodies(container)).toEqual([
      "can we meet?",
      "I'm away until Tuesday",
      "sure, which day?",
      "Friday or Monday",
    ]);
    expect(
      screen.getByText(/It looks automated to me: it was sent automatically/),
    ).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /earlier/ })).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "Hide what came before" }));
    expect(screen.queryByText("can we meet?")).toBeNull();
    expect(
      screen.getByRole("button", { name: "Show the 4 messages before this one" }),
    ).toBeInTheDocument();
  });

  it("reads what came after it on from the newest message shown, and adds it below", async () => {
    const first: ConversationPage = {
      messages: [
        message({ id: "m6", body: "Out of office until Monday", automated: "auto_reply" }),
      ],
      more: 1,
    };
    const next: ConversationPage = {
      messages: [message({ id: "m7", body: "(no sender)", direction: "unknown", author: null })],
      more: 0,
    };
    native.conversationPage.mockResolvedValueOnce(first).mockResolvedValueOnce(next);
    const { container } = renderIt("later", 2);

    fireEvent.click(screen.getByRole("button", { name: "Show the 2 messages after this one" }));
    expect(await screen.findByText("Out of office until Monday")).toBeInTheDocument();
    expect(native.conversationPage).toHaveBeenCalledWith("c1", "m5", "later", 20);

    fireEvent.click(screen.getByRole("button", { name: "Show 1 later message" }));
    expect(await screen.findByText("(no sender)")).toBeInTheDocument();
    expect(native.conversationPage).toHaveBeenLastCalledWith("c1", "m6", "later", 20);
    expect(bodies(container)).toEqual(["Out of office until Monday", "(no sender)"]);
    expect(screen.getByText("I couldn't tell who wrote this")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Hide what came after" })).toBeInTheDocument();
  });

  it("puts away what it showed when part of the conversation is deleted, and reads it again", async () => {
    // As the native side reports a message no longer in the database: here
    // the far end of the page already shown, deleted before reading further.
    native.conversationPage
      .mockResolvedValueOnce(nearest)
      .mockRejectedValueOnce(
        Object.assign(new Error("not found: message m3 in c1"), { code: "not_found" }),
      )
      .mockResolvedValueOnce({ ...nearest, more: 0 });
    renderIt("earlier", 4);
    fireEvent.click(screen.getByRole("button", { name: "Show the 4 messages before this one" }));
    expect(await screen.findByText("sure, which day?")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Show 2 earlier messages" }));
    expect(await screen.findByText(GONE)).toBeInTheDocument();
    expect(screen.queryByText("sure, which day?")).toBeNull();
    expect(screen.queryByText(/not found/)).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "Read it again" }));
    expect(await screen.findByText("sure, which day?")).toBeInTheDocument();
    expect(native.conversationPage).toHaveBeenLastCalledWith("c1", "m5", "earlier", 20);
    expect(screen.queryByText(GONE)).toBeNull();
    expect(screen.queryByRole("button", { name: /earlier/ })).toBeNull();
  });

  it("shows any other failure as it is", async () => {
    native.conversationPage.mockRejectedValueOnce(
      Object.assign(new Error("database is locked"), { code: "db" }),
    );
    renderIt("later", 1);
    fireEvent.click(screen.getByRole("button", { name: "Show the message after this one" }));
    expect(await screen.findByText("database is locked")).toBeInTheDocument();
  });
});
