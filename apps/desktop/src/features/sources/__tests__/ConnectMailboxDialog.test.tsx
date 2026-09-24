import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  MICROSOFT_UNAVAILABLE,
  type CredentialProtection,
  type ImapProbe,
  type ProviderState,
} from "@mimic/contracts";
import { qk } from "@/app/queryClient";
import { ConnectMailboxDialog } from "../ConnectMailboxDialog";

// The real hook and query client run; only the native call is replaced.
const native = vi.hoisted(() => ({
  providerState: vi.fn(),
  mailSignInAvailable: vi.fn(),
  signInToMailbox: vi.fn(),
  connectSignedInMailbox: vi.fn(),
  cancelMailSignIn: vi.fn(),
}));
vi.mock("@/lib/ipc", () => ({ ipc: native }));

const state = (protection: CredentialProtection): ProviderState => ({
  providers: [],
  active: null,
  configuredSecrets: [],
  credentials: { protection, unsealedLeft: false, locked: [], unreadable: null },
});

function renderDialog(onClose = () => {}) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <ConnectMailboxDialog onClose={onClose} />
    </QueryClientProvider>,
  );
  return client;
}

describe("the mailbox dialog says how the password will be kept", () => {
  beforeEach(() => {
    for (const f of Object.values(native)) f.mockReset();
    native.mailSignInAvailable.mockResolvedValue(false);
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

describe("a Microsoft mailbox signs in with Microsoft", () => {
  const found: ImapProbe = {
    folders: ["INBOX", "Sent Items"],
    sentFolder: "Sent Items",
    counts: [
      ["INBOX", 3],
      ["Sent Items", 2],
    ],
    warnings: [],
  };

  beforeEach(() => {
    for (const f of Object.values(native)) f.mockReset();
    native.providerState.mockResolvedValue(state("account"));
    native.mailSignInAvailable.mockResolvedValue(true);
    native.cancelMailSignIn.mockResolvedValue(undefined);
  });

  function typeAddress(address: string) {
    fireEvent.change(screen.getByLabelText("Your email address"), { target: { value: address } });
  }

  it("asks for no password, signs in, shows what it found, and connects it", async () => {
    native.signInToMailbox.mockResolvedValue(found);
    native.connectSignedInMailbox.mockResolvedValue({});
    const onClose = vi.fn();
    renderDialog(onClose);
    typeAddress("c@outlook.com");
    expect(await screen.findByText(/I never see your password/)).toBeInTheDocument();
    expect(screen.queryByLabelText("App password")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Sign in with Microsoft" }));
    await waitFor(() => expect(native.signInToMailbox).toHaveBeenCalledWith("c@outlook.com"));
    expect(
      await screen.findByText("I’ll read INBOX (3 messages) and Sent Items (2 messages)."),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Connect and start reading" }));
    await waitFor(() =>
      expect(native.connectSignedInMailbox).toHaveBeenCalledWith("c@outlook.com", true),
    );
    await waitFor(() => expect(onClose).toHaveBeenCalled());
  });

  it("can be stopped while it waits on the browser, and says nothing about being stopped", async () => {
    let fail: (e: unknown) => void = () => {};
    native.signInToMailbox.mockReturnValue(new Promise((_, reject) => (fail = reject)));
    renderDialog();
    typeAddress("c@hotmail.com");
    await screen.findByText(/I never see your password/);
    fireEvent.click(screen.getByRole("button", { name: "Sign in with Microsoft" }));
    expect(await screen.findByText(/Finish signing in in your browser/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Stop signing in" }));
    expect(native.cancelMailSignIn).toHaveBeenCalled();
    fail(Object.assign(new Error("Signing in was stopped."), { code: "canceled" }));
    expect(await screen.findByRole("button", { name: "Sign in with Microsoft" })).toBeEnabled();
    expect(screen.queryByText("Signing in was stopped.")).toBeNull();
  });

  it("offers Microsoft 365 work accounts the same, under server settings", async () => {
    native.signInToMailbox.mockResolvedValue(found);
    renderDialog();
    typeAddress("c@formicaria.us");
    expect(await screen.findByLabelText("App password")).toBeInTheDocument();
    fireEvent.click(await screen.findByLabelText(/This is a Microsoft account/));
    expect(screen.queryByLabelText("App password")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Sign in with Microsoft" }));
    await waitFor(() => expect(native.signInToMailbox).toHaveBeenCalledWith("c@formicaria.us"));
  });

  it("says so, and offers nothing that would fail, when this copy can't sign in with Microsoft", async () => {
    native.mailSignInAvailable.mockResolvedValue(false);
    renderDialog();
    typeAddress("c@outlook.com");
    expect(await screen.findByText(MICROSOFT_UNAVAILABLE)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Sign in with Microsoft" })).toBeDisabled();
    expect(screen.queryByLabelText("App password")).toBeNull();
  });
});
