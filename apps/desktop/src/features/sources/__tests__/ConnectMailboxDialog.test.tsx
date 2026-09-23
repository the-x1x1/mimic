import { render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { CredentialProtection, ProviderState } from "@mimic/contracts";
import { qk } from "@/app/queryClient";
import { ConnectMailboxDialog } from "../ConnectMailboxDialog";

// The real hook and query client run; only the native call is replaced.
const native = vi.hoisted(() => ({ providerState: vi.fn() }));
vi.mock("@/lib/ipc", () => ({ ipc: native }));

const state = (protection: CredentialProtection): ProviderState => ({
  providers: [],
  active: null,
  configuredSecrets: [],
  credentials: { protection, unsealedLeft: false, locked: [], unreadable: null },
});

function renderDialog() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <ConnectMailboxDialog onClose={() => {}} />
    </QueryClientProvider>,
  );
  return client;
}

describe("the mailbox dialog says how the password will be kept", () => {
  beforeEach(() => {
    native.providerState.mockReset();
  });

  it("claims nothing until the store answers, then says it is locked to the account", async () => {
    let answer: (s: ProviderState) => void = () => {};
    native.providerState.mockReturnValue(new Promise<ProviderState>((r) => (answer = r)));
    renderDialog();
    expect(screen.getByText(/It stays on this computer\./)).toBeInTheDocument();
    expect(screen.queryByText(/locked to your Windows account/)).toBeNull();

    answer(state("account"));
    expect(
      await screen.findByText(/It stays on this computer, locked to your Windows account\./),
    ).toBeInTheDocument();
  });

  it("claims nothing more when the store does not seal", async () => {
    native.providerState.mockResolvedValue(state("file"));
    const client = renderDialog();
    await waitFor(() => expect(client.getQueryState(qk.providers)?.status).toBe("success"));
    expect(screen.getByText(/It stays on this computer\./)).toBeInTheDocument();
    expect(screen.queryByText(/locked to your Windows account/)).toBeNull();
  });
});
