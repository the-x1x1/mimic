import { readFileSync } from "node:fs";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { MemoryRouter } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { GenerationContext, type Draft } from "@mimic/contracts";
import { contractFixture } from "@mimic/test-fixtures";
import { useAnywhere } from "@/state/anywhere";
import { ComposePanel } from "../ComposePanel";

// The real hooks run; only the native calls are replaced.
const native = vi.hoisted(() => ({
  people: vi.fn(),
  providerState: vi.fn(),
  checkProvider: vi.fn(),
  generationContext: vi.fn(),
  generateDraft: vi.fn(),
  resolveDraft: vi.fn(),
  stepAside: vi.fn(),
}));
vi.mock("@/lib/ipc", () => ({ ipc: native }));

const context = GenerationContext.parse(
  JSON.parse(readFileSync(contractFixture("generation_context.json"), "utf8")),
);

const draft = (id: string, text: string): Draft => ({
  id,
  participantId: null,
  conversationId: null,
  channel: "email",
  situationId: null,
  incomingMessage: null,
  intent: "yes, thursday",
  generatedText: text,
  finalText: null,
  provider: "local",
  model: "llama3.2:3b",
  context: {},
  promptHash: "h",
  evidence: {},
  createdAt: "2026-09-23T10:00:00Z",
  resolvedAt: null,
  outcome: null,
  incomingMessageId: null,
  alternativeTo: null,
});

describe("writing something new", () => {
  beforeEach(() => {
    for (const f of Object.values(native)) f.mockReset();
    native.people.mockResolvedValue({ people: [], peopleTotal: 0 });
    native.providerState.mockResolvedValue({
      providers: [{ id: "local", displayName: "Local model", local: true }],
      active: "local",
      configuredSecrets: [],
    });
    native.checkProvider.mockResolvedValue(undefined);
    native.generationContext.mockResolvedValue(context);
    native.resolveDraft.mockImplementation(async (id: string, outcome: string) => ({
      ...draft(id, ""),
      outcome,
    }));
  });

  it("records a draft it replaced as passed over, not left waiting", async () => {
    native.generateDraft
      .mockResolvedValueOnce(draft("d1", "yes, thursday works"))
      .mockResolvedValueOnce(draft("d2", "thursday!"));
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(
      <QueryClientProvider client={client}>
        <MemoryRouter>
          <ComposePanel />
        </MemoryRouter>
      </QueryClientProvider>,
    );
    const write = await screen.findByRole("button", { name: "Write a draft" });
    await waitFor(() => expect(write).toBeEnabled());
    fireEvent.click(write);
    expect(await screen.findByDisplayValue("yes, thursday works")).toBeInTheDocument();
    expect(native.resolveDraft).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "Shorter" }));
    expect(await screen.findByDisplayValue("thursday!")).toBeInTheDocument();
    expect(native.resolveDraft).toHaveBeenCalledWith("d1", "regenerated", null);
    expect(native.resolveDraft).toHaveBeenCalledTimes(1);
  });

  it("opens on a message copied from anywhere, as the message being answered", async () => {
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(
      <QueryClientProvider client={client}>
        <MemoryRouter
          initialEntries={[{ pathname: "/write", state: { pasted: "are we still on for 7?" } }]}
        >
          <ComposePanel />
        </MemoryRouter>
      </QueryClientProvider>,
    );
    expect(await screen.findByDisplayValue("are we still on for 7?")).toBeInTheDocument();
    await waitFor(() =>
      expect(native.generationContext).toHaveBeenCalledWith(
        expect.objectContaining({ incomingMessage: "are we still on for 7?" }),
      ),
    );
  });

  describe("answered from anywhere", () => {
    const writeText = vi.fn();

    beforeEach(() => {
      useAnywhere.setState({ copied: null });
      writeText.mockReset();
      writeText.mockResolvedValue(undefined);
      Object.defineProperty(navigator, "clipboard", {
        value: { writeText },
        configurable: true,
      });
      native.stepAside.mockResolvedValue(true);
      native.generateDraft.mockResolvedValue(draft("d1", "yes, see you at 7"));
    });

    async function draftFromAnywhere() {
      const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
      render(
        <QueryClientProvider client={client}>
          <MemoryRouter
            initialEntries={[{ pathname: "/write", state: { pasted: "are we still on for 7?" } }]}
          >
            <ComposePanel />
          </MemoryRouter>
        </QueryClientProvider>,
      );
      const write = await screen.findByRole("button", { name: "Write a draft" });
      await waitFor(() => expect(write).toBeEnabled());
      fireEvent.click(write);
      expect(await screen.findByDisplayValue("yes, see you at 7")).toBeInTheDocument();
    }

    it("copies the reply and steps aside, then asks whether it went", async () => {
      await draftFromAnywhere();
      expect(screen.queryByRole("button", { name: "Copy" })).toBeNull();
      fireEvent.change(screen.getByDisplayValue("yes, see you at 7"), {
        target: { value: "yes! see you at 7" },
      });
      fireEvent.click(screen.getByRole("button", { name: "Copy and go back" }));

      await waitFor(() => expect(native.stepAside).toHaveBeenCalled());
      expect(writeText).toHaveBeenCalledWith("yes! see you at 7");
      expect(native.resolveDraft).not.toHaveBeenCalled();
      expect(await screen.findByText("The reply you copied")).toBeInTheDocument();
      expect(screen.getByText("yes! see you at 7")).toBeInTheDocument();

      fireEvent.click(screen.getByRole("button", { name: "It went as I copied it" }));
      await waitFor(() =>
        expect(native.resolveDraft).toHaveBeenCalledWith("d1", "sent_edited", "yes! see you at 7"),
      );
      await waitFor(() => expect(screen.queryByText("The reply you copied")).toBeNull());
    });

    it("records a reply that didn't go, and nothing for one changed after copying", async () => {
      await draftFromAnywhere();
      fireEvent.click(screen.getByRole("button", { name: "Copy and go back" }));
      fireEvent.click(await screen.findByRole("button", { name: "It didn’t go" }));
      await waitFor(() =>
        expect(native.resolveDraft).toHaveBeenCalledWith("d1", "discarded", null),
      );

      useAnywhere.setState({
        copied: { draftId: "d2", text: "sure", generatedText: "sure" },
      });
      fireEvent.click(await screen.findByRole("button", { name: "I changed it first" }));
      await waitFor(() => expect(screen.queryByText("The reply you copied")).toBeNull());
      expect(native.resolveDraft).toHaveBeenCalledTimes(1);
    });
  });
});
