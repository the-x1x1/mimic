import { readFileSync } from "node:fs";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { MemoryRouter } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { GenerationContext, type Draft } from "@mimic/contracts";
import { contractFixture } from "@mimic/test-fixtures";
import { ComposePanel } from "../ComposePanel";

// The real hooks run; only the native calls are replaced.
const native = vi.hoisted(() => ({
  people: vi.fn(),
  providerState: vi.fn(),
  checkProvider: vi.fn(),
  generationContext: vi.fn(),
  generateDraft: vi.fn(),
  resolveDraft: vi.fn(),
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
});
