import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { FoundMessage, SearchPage } from "@mimic/contracts";
import { FindPage } from "../FindPage";

// The real queries run; only the native calls are replaced.
const native = vi.hoisted(() => ({ searchMessages: vi.fn(), conversationPage: vi.fn() }));
vi.mock("@/lib/ipc", () => ({ ipc: native }));

const found = (
  id: string,
  body: string,
  day: number,
  over: Partial<FoundMessage> = {},
): FoundMessage => ({
  message: {
    id,
    direction: "other",
    author: "Ada Lovelace",
    sentAt: `2026-03-0${day}T10:00:00Z`,
    body,
    automated: null,
    filing: null,
  },
  conversationId: "c1",
  subject: "Lunch",
  snippet: [
    { text: "Drinks on ", hit: false },
    { text: "Friday", hit: true },
    { text: "?", hit: false },
  ],
  earlier: 0,
  later: 0,
  ...over,
});

const first: SearchPage = {
  found: [
    found("m3", "Drinks on Friday? Or Saturday", 3, { earlier: 2, later: 1 }),
    found("m2", "Drinks on Friday?", 2, { subject: null }),
  ],
  more: true,
  total: 3,
  capped: false,
};
const second: SearchPage = {
  found: [found("m1", "Friday then", 1)],
  more: false,
  total: 3,
  capped: false,
};

function renderIt() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <FindPage />
    </QueryClientProvider>,
  );
}

function search(words: string) {
  fireEvent.change(screen.getByRole("searchbox", { name: "Words to find" }), {
    target: { value: words },
  });
  fireEvent.click(screen.getByRole("button", { name: "Find" }));
}

describe("finding what was said", () => {
  beforeEach(() => {
    native.searchMessages.mockReset();
    native.conversationPage.mockReset();
  });

  it("finds what was typed, says how much, marks the match, and reads on from the last one", async () => {
    native.searchMessages.mockResolvedValueOnce(first).mockResolvedValueOnce(second);
    renderIt();
    expect(native.searchMessages).not.toHaveBeenCalled();
    // The live region is there before anything is said in it, so what is
    // said is announced.
    expect(screen.getByRole("status")).toHaveTextContent("");
    search("  friday ");

    await waitFor(() =>
      expect(screen.getByRole("status")).toHaveTextContent(
        "3 messages say that, the newest first.",
      ),
    );
    expect(native.searchMessages).toHaveBeenCalledWith("friday", "anyone", null, 20);
    const items = screen.getAllByRole("listitem");
    expect(items).toHaveLength(2);
    expect(within(items[0]!).getByText("Friday").tagName).toBe("MARK");
    expect(items[0]).toHaveTextContent("Ada wrote");
    expect(items[0]).toHaveTextContent("in Lunch");
    expect(items[1]).toHaveTextContent("in No subject");

    fireEvent.click(screen.getByRole("button", { name: "Show older ones" }));
    await waitFor(() => expect(screen.getAllByRole("listitem")).toHaveLength(3));
    // The button goes with the last page; the keyboard goes to what it brought.
    await waitFor(() => expect(document.activeElement).toBe(screen.getAllByRole("listitem")[2]));
    expect(native.searchMessages).toHaveBeenLastCalledWith(
      "friday",
      "anyone",
      { at: "2026-03-02T10:00:00Z", id: "m2" },
      20,
    );
    expect(screen.queryByRole("button", { name: "Show older ones" })).toBeNull();
  });

  it("looks for nothing when nothing typed is a word", () => {
    renderIt();
    search(" ?! ");
    expect(screen.getByRole("status")).toHaveTextContent("Type a word or two to look for.");
    expect(native.searchMessages).not.toHaveBeenCalled();
  });

  it("looks again, through whoever's messages are chosen", async () => {
    native.searchMessages.mockResolvedValue({ ...second, total: 1 });
    renderIt();
    search("friday");
    await waitFor(() =>
      expect(screen.getByRole("status")).toHaveTextContent("One message says that."),
    );
    fireEvent.change(screen.getByRole("combobox", { name: "Written by" }), {
      target: { value: "you" },
    });
    await waitFor(() =>
      expect(native.searchMessages).toHaveBeenLastCalledWith("friday", "you", null, 20),
    );
    await waitFor(() =>
      expect(screen.getByRole("status")).toHaveTextContent("One of your messages says that."),
    );
  });

  it("shows a message found in its conversation, reading the rest only when asked", async () => {
    native.searchMessages.mockResolvedValue(first);
    renderIt();
    search("friday");
    const item = (await screen.findAllByRole("listitem"))[0]!;
    const toggle = within(item).getByRole("button", { name: "Show it in its conversation" });
    // Every result has one; which message it is about is said with it.
    expect(toggle).toHaveAccessibleDescription(/Ada wrote.*in Lunch/);
    fireEvent.click(toggle);
    expect(within(item).getByText("Drinks on Friday? Or Saturday")).toBeInTheDocument();
    expect(
      within(item).getByRole("button", { name: "Show the 2 messages before this one" }),
    ).toBeInTheDocument();
    expect(
      within(item).getByRole("button", { name: "Show the message after this one" }),
    ).toBeInTheDocument();
    expect(native.conversationPage).not.toHaveBeenCalled();
    fireEvent.click(within(item).getByRole("button", { name: "Hide the conversation" }));
    expect(within(item).queryByText("Drinks on Friday? Or Saturday")).toBeNull();
  });

  it("clears the words on Escape without closing Find, and lets an Escape with nothing to clear through", () => {
    renderIt();
    const box = screen.getByRole("searchbox", { name: "Words to find" });
    fireEvent.change(box, { target: { value: "friday" } });
    // Handled: the drawer around Find leaves a handled Escape alone.
    expect(fireEvent.keyDown(box, { key: "Escape" })).toBe(false);
    expect(box).toHaveValue("");
    expect(fireEvent.keyDown(box, { key: "Escape" })).toBe(true);
  });
});
