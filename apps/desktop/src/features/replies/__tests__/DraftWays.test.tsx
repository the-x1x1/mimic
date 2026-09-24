import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Draft } from "@mimic/contracts";
import { DraftReview } from "../RepliesPage";

// The real hooks run; only the native calls are replaced.
const native = vi.hoisted(() => ({
  writeAnotherDraft: vi.fn(),
  resolveDraft: vi.fn(),
  addDraftPreference: vi.fn(),
}));
vi.mock("@/lib/ipc", () => ({ ipc: native }));

const draft = (over: Partial<Draft> = {}): Draft => ({
  id: "d1",
  participantId: "p1",
  conversationId: "c1",
  channel: "email",
  situationId: null,
  incomingMessage: "can you confirm Friday?",
  intent: "yes, and I'll bring the numbers",
  generatedText: "yes, friday works. i'll bring the numbers",
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
  alternativeTo: null,
  ...over,
});

const shorter = draft({
  id: "d2",
  alternativeTo: "d1",
  generatedText: "friday works!",
  context: { adjustment: "shorter" },
});
const casual = draft({
  id: "d3",
  alternativeTo: "d1",
  generatedText: "yep friday, numbers in hand",
  context: { adjustment: "moreCasual" },
});

function renderReview(alternatives: Draft[] = [], onDone = vi.fn()) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <DraftReview draft={draft()} alternatives={alternatives} onDone={onDone} />
    </QueryClientProvider>,
  );
  return onDone;
}

const ways = () => screen.getByRole("group", { name: "Write it another way" });

describe("other ways of saying a draft", () => {
  beforeEach(() => {
    for (const f of Object.values(native)) f.mockReset();
    native.resolveDraft.mockImplementation(async (id: string, outcome: string) => ({
      ...draft({ id }),
      outcome,
    }));
    Object.defineProperty(navigator, "clipboard", {
      value: { writeText: vi.fn().mockResolvedValue(undefined) },
      configurable: true,
    });
  });

  it("writes one when asked, and shows it beside the first, named by how it differs", async () => {
    native.writeAnotherDraft.mockResolvedValue(shorter);
    renderReview();
    expect(
      within(ways())
        .getAllByRole("button")
        .map((b) => b.textContent),
    ).toEqual(["Shorter", "Longer", "More casual", "More professional"]);
    expect(screen.queryByRole("group", { name: "As I first wrote it" })).toBeNull();

    fireEvent.click(within(ways()).getByRole("button", { name: "Shorter" }));
    await waitFor(() => expect(native.writeAnotherDraft).toHaveBeenCalledWith("d1", "shorter"));
    const first = await screen.findByRole("group", { name: "As I first wrote it" });
    expect(first).toHaveTextContent("yes, friday works");
    expect(screen.getByRole("group", { name: "Shorter" })).toHaveTextContent("friday works!");
    // One of each kind; the first is put aside only with the rest.
    expect(within(ways()).queryByRole("button", { name: "Shorter" })).toBeNull();
    expect(within(first).queryByRole("button", { name: "Not this one" })).toBeNull();
    expect(screen.getByRole("button", { name: "None of these" })).toBeInTheDocument();
    // Focus goes to what arrived, and the status line says so.
    expect(screen.getByText("Shorter", { selector: ".letter-label" })).toHaveFocus();
    expect(screen.getByRole("status")).toHaveTextContent("Here it is shorter, beside the first.");
  });

  it("uses the words of the way chosen, as changed, and is done", async () => {
    const onDone = renderReview([shorter]);
    const way = screen.getByRole("group", { name: "Shorter" });
    fireEvent.click(within(way).getByRole("button", { name: "Change it" }));
    // Asked for shorter, so what is changed in it teaches nothing, and it says so.
    expect(within(way).getByText(/won’t change how I write/)).toBeInTheDocument();
    fireEvent.change(within(way).getByRole("textbox"), {
      target: { value: "friday works, see you" },
    });
    fireEvent.click(within(way).getByRole("button", { name: "Use this" }));
    await waitFor(() => expect(onDone).toHaveBeenCalled());
    expect(native.resolveDraft).toHaveBeenCalledWith("d2", "sent_edited", "friday works, see you");
    expect(navigator.clipboard.writeText).toHaveBeenCalledWith("friday works, see you");
  });

  it("puts one other way aside by itself, and every way aside with None of these", async () => {
    const onDone = renderReview([shorter, casual]);
    fireEvent.click(
      within(screen.getByRole("group", { name: "Shorter" })).getByRole("button", {
        name: "Not this one",
      }),
    );
    await waitFor(() => expect(screen.queryByRole("group", { name: "Shorter" })).toBeNull());
    expect(native.resolveDraft).toHaveBeenCalledWith("d2", "discarded", null);
    expect(screen.getByRole("group", { name: "More casual" })).toBeInTheDocument();
    expect(screen.getByText("More casual", { selector: ".letter-label" })).toHaveFocus();
    expect(screen.getByRole("status")).toHaveTextContent("Put aside: Shorter.");
    expect(within(ways()).getByRole("button", { name: "Shorter" })).toBeInTheDocument();
    expect(onDone).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "None of these" }));
    await waitFor(() => expect(onDone).toHaveBeenCalled());
    expect(native.resolveDraft).toHaveBeenLastCalledWith("d1", "discarded", null);
  });

  it("keeps what was typed into the first draft while other ways come and go", async () => {
    native.writeAnotherDraft.mockResolvedValue(casual);
    renderReview();
    fireEvent.click(screen.getByRole("button", { name: "Change it" }));
    fireEvent.change(screen.getByRole("textbox"), {
      target: { value: "yes, friday. numbers too" },
    });
    fireEvent.click(within(ways()).getByRole("button", { name: "More casual" }));
    await screen.findByRole("group", { name: "More casual" });
    const first = screen.getByRole("group", { name: "As I first wrote it" });
    expect(within(first).getByRole("textbox")).toHaveValue("yes, friday. numbers too");

    fireEvent.click(
      within(screen.getByRole("group", { name: "More casual" })).getByRole("button", {
        name: "Not this one",
      }),
    );
    await waitFor(() => expect(screen.queryByRole("group", { name: "More casual" })).toBeNull());
    const alone = screen.getByRole("group", { name: "I’d say" });
    expect(within(alone).getByRole("textbox")).toHaveValue("yes, friday. numbers too");
    expect(screen.getByText("I’d say", { selector: ".letter-label" })).toHaveFocus();
  });

  it("says why a way couldn't be put aside", async () => {
    native.resolveDraft.mockRejectedValue(new Error("That draft was already used or put aside."));
    renderReview([shorter]);
    fireEvent.click(
      within(screen.getByRole("group", { name: "Shorter" })).getByRole("button", {
        name: "Not this one",
      }),
    );
    expect(
      await screen.findByText("That draft was already used or put aside."),
    ).toBeInTheDocument();
    expect(screen.getByRole("group", { name: "Shorter" })).toBeInTheDocument();
  });

  it("says why when another way can't be written, and keeps what is there", async () => {
    native.writeAnotherDraft.mockRejectedValue(new Error("The local model isn't answering."));
    renderReview();
    fireEvent.click(within(ways()).getByRole("button", { name: "Longer" }));
    expect(await screen.findByText("The local model isn't answering.")).toBeInTheDocument();
    expect(screen.getByText("yes, friday works. i'll bring the numbers")).toBeInTheDocument();
  });
});
