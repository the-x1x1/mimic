import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { OutboxEntry } from "@mimic/contracts";
import { SentList } from "../SentList";

// The real hooks run; only the native calls are replaced.
const native = vi.hoisted(() => ({ sent: vi.fn(), replyDidNotGo: vi.fn() }));
vi.mock("@/lib/ipc", () => ({ ipc: native }));

const entry = (over: Partial<OutboxEntry>): OutboxEntry => ({
  id: "o1",
  draftId: "d1",
  sourceId: "s1",
  conversationId: "c1",
  answersMessageId: "m1",
  participantId: "p1",
  fromAddress: "c@example.com",
  to: ["ada@example.com"],
  cc: [],
  subject: "Re: Lunch",
  body: "perfect, see you there",
  messageId: "x@example.com",
  how: "approved",
  status: "sent",
  sendAfter: "2026-10-26T09:00:00Z",
  error: null,
  createdAt: "2026-10-26T09:00:00Z",
  updatedAt: "2026-10-26T09:00:01Z",
  sentAt: "2026-10-26T09:00:01Z",
  ...over,
});

function renderIt() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <SentList />
    </QueryClientProvider>,
  );
}

describe("what was sent", () => {
  beforeEach(() => {
    for (const f of Object.values(native)) f.mockReset();
  });

  it("lists each reply with where it went, and why one didn't", async () => {
    native.sent.mockResolvedValue([
      entry({}),
      entry({
        id: "o2",
        subject: "Re: Numbers",
        status: "failed",
        sentAt: null,
        error: "The mail server wouldn't send to bob@example.com: 550 No such user here",
      }),
    ]);
    renderIt();
    expect(await screen.findByText("Sent to ada@example.com.")).toBeInTheDocument();
    expect(
      screen.getByText(/Didn't go to ada@example.com: The mail server wouldn't send/),
    ).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Sent folder/ })).toBeNull();
  });

  it("asks about a reply that may have gone, and frees it once the user says it didn't", async () => {
    native.sent.mockResolvedValue([entry({ status: "unsure", sentAt: null })]);
    native.replyDidNotGo.mockResolvedValue(entry({ status: "failed" }));
    renderIt();
    expect(await screen.findByText(/May have gone to ada@example.com/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "It isn’t in my Sent folder" }));
    await waitFor(() => expect(native.replyDidNotGo).toHaveBeenCalledWith("o1"));
  });

  it("shows nothing until something was sent", async () => {
    native.sent.mockResolvedValue([]);
    renderIt();
    await waitFor(() => expect(native.sent).toHaveBeenCalled());
    expect(screen.queryByText("What I've sent")).toBeNull();
  });
});
