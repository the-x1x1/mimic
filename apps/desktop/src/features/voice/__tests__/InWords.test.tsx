import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ProviderState } from "@mimic/contracts";
import { InWords } from "../InWords";

// The real hooks run; only the native calls are replaced.
const native = vi.hoisted(() => ({
  providerState: vi.fn(),
  startDescribingVoice: vi.fn(),
  jobs: vi.fn(),
  checkProvider: vi.fn(),
}));
vi.mock("@/lib/ipc", () => ({ ipc: native }));

const state = (local: boolean): ProviderState =>
  ({
    providers: [
      {
        id: "p",
        displayName: local ? "Local model" : "Cloud model",
        local,
        model: "m",
        description: "",
        requiresCredential: false,
      },
    ],
    active: "p",
  }) as unknown as ProviderState;

function renderIt(measured: number) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <InWords measured={measured} />
    </QueryClientProvider>,
  );
}

describe("how you write, in words", () => {
  beforeEach(() => {
    for (const f of Object.values(native)) f.mockReset();
    native.jobs.mockResolvedValue([]);
    native.startDescribingVoice.mockResolvedValue({ id: "j1" });
    native.checkProvider.mockResolvedValue(undefined);
  });

  it("says where the numbers go, and that nothing else does, before starting", async () => {
    native.providerState.mockResolvedValue(state(false));
    renderIt(3);
    expect(
      await screen.findByText(
        /Only the numbers go to Cloud model, which is not on this computer, with the greetings and sign-offs you use from my own short list \(“hi”, “thanks”\) — never a message/,
      ),
    ).toBeInTheDocument();
    const button = screen.getByRole("button", { name: "Put it into words" });
    await waitFor(() => expect(button).toBeEnabled());
    fireEvent.click(button);
    await waitFor(() => expect(native.startDescribingVoice).toHaveBeenCalled());
  });

  it("names a model on this computer as that", async () => {
    native.providerState.mockResolvedValue(state(true));
    renderIt(1);
    expect(
      await screen.findByText(/Only the numbers go to Local model, which runs on this computer/),
    ).toBeInTheDocument();
  });

  it("says so, and offers nothing, when the chosen model isn't answering", async () => {
    native.providerState.mockResolvedValue(state(true));
    native.checkProvider.mockRejectedValue(
      new Error("provider is not reachable: connection refused"),
    );
    renderIt(2);
    expect(
      await screen.findByText(
        /Local model isn’t answering \(provider is not reachable: connection refused\)/,
      ),
    ).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Put it into words" })).toBeNull();
  });

  it("is not offered before anything is measured", () => {
    native.providerState.mockResolvedValue(state(true));
    renderIt(0);
    expect(screen.queryByText("In words")).toBeNull();
  });
});
