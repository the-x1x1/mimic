import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { MemoryRouter } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { SituationFiling } from "@mimic/contracts";
import { WhatEachIsDoing } from "../WhatEachIsDoing";

// The real hooks run; only the native calls are replaced.
const native = vi.hoisted(() => ({
  situationFiling: vi.fn(),
  startReadingSituations: vi.fn(),
  jobs: vi.fn(),
  checkProvider: vi.fn(),
}));
vi.mock("@/lib/ipc", () => ({ ipc: native }));

function renderIt(filing: SituationFiling) {
  native.situationFiling.mockResolvedValue(filing);
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <MemoryRouter>
        <WhatEachIsDoing />
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

describe("how your messages were filed by what they were doing", () => {
  beforeEach(() => {
    for (const f of Object.values(native)) f.mockReset();
    native.jobs.mockResolvedValue([]);
    native.startReadingSituations.mockResolvedValue({ id: "j1" });
    native.checkProvider.mockResolvedValue(undefined);
  });

  it("counts each hand, and has the model on this computer read the rest when asked", async () => {
    renderIt({
      byRules: 120,
      byModel: 30,
      byYou: 1,
      localProvider: "local",
      localModel: "llama3.2:3b",
    });
    expect(
      await screen.findByText(
        /120 messages left to the rules; 30 messages read by the model on this computer; 1 message you said yourself\./,
      ),
    ).toBeInTheDocument();
    expect(
      screen.getByText(/runs on this computer, so reading them sends nothing anywhere/),
    ).toBeInTheDocument();
    const button = screen.getByRole("button", { name: "Have llama3.2:3b read them" });
    await waitFor(() => expect(button).toBeEnabled());
    expect(native.checkProvider).toHaveBeenCalledWith("local");
    fireEvent.click(button);
    await waitFor(() => expect(native.startReadingSituations).toHaveBeenCalled());
  });

  it("sends nothing anywhere without a model on this computer", async () => {
    renderIt({ byRules: 12, byModel: 0, byYou: 0, localProvider: null, localModel: null });
    expect(
      await screen.findByText(/only do that with a model on this computer/),
    ).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /read them/ })).toBeNull();
    expect(screen.getByRole("link", { name: "Settings" })).toHaveAttribute("href", "/settings");
  });

  it("says so while a reading is under way", async () => {
    native.jobs.mockResolvedValue([{ id: "j1", type: "read_situations", status: "running" }]);
    renderIt({
      byRules: 5,
      byModel: 0,
      byYou: 0,
      localProvider: "local",
      localModel: "llama3.2:3b",
    });
    expect(await screen.findByRole("button", { name: "Reading them…" })).toBeDisabled();
  });

  it("says so, and offers nothing, when the model on this computer isn't answering", async () => {
    native.checkProvider.mockRejectedValue(
      new Error("provider is not reachable: connection refused"),
    );
    renderIt({
      byRules: 12,
      byModel: 0,
      byYou: 0,
      localProvider: "local",
      localModel: "llama3.2:3b",
    });
    expect(
      await screen.findByText(
        /it isn’t answering \(provider is not reachable: connection refused\)/,
      ),
    ).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /read them/ })).toBeNull();
  });

  it("has nothing to read when nothing is left to the rules", async () => {
    renderIt({
      byRules: 0,
      byModel: 40,
      byYou: 2,
      localProvider: "local",
      localModel: "llama3.2:3b",
    });
    expect(
      await screen.findByRole("button", { name: "Have llama3.2:3b read them" }),
    ).toBeDisabled();
  });
});
