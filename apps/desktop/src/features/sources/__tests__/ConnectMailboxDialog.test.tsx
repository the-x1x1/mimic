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
  mailExchangers: vi.fn(),
  reachMailServers: vi.fn(),
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
    native.mailSignInAvailable.mockResolvedValue({ microsoft: false, google: false });
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
    native.mailSignInAvailable.mockResolvedValue({ microsoft: true, google: false });
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
    await waitFor(() =>
      expect(native.signInToMailbox).toHaveBeenCalledWith("c@outlook.com", "microsoft"),
    );
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
    await waitFor(() =>
      expect(native.signInToMailbox).toHaveBeenCalledWith("c@formicaria.us", "microsoft"),
    );
  });

  it("says so, and offers nothing that would fail, when this copy can't sign in with Microsoft", async () => {
    native.mailSignInAvailable.mockResolvedValue({ microsoft: false, google: false });
    renderDialog();
    typeAddress("c@outlook.com");
    expect(await screen.findByText(MICROSOFT_UNAVAILABLE)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Sign in with Microsoft" })).toBeDisabled();
    expect(screen.queryByLabelText("App password")).toBeNull();
  });
});

describe("a Gmail or Google Workspace mailbox can sign in with Google", () => {
  const found: ImapProbe = {
    folders: ["INBOX", "[Gmail]/Sent Mail"],
    sentFolder: "[Gmail]/Sent Mail",
    counts: [
      ["INBOX", 4],
      ["[Gmail]/Sent Mail", 1],
    ],
    warnings: [],
  };

  beforeEach(() => {
    for (const f of Object.values(native)) f.mockReset();
    native.providerState.mockResolvedValue(state("account"));
    native.mailSignInAvailable.mockResolvedValue({ microsoft: false, google: true });
    native.cancelMailSignIn.mockResolvedValue(undefined);
  });

  function typeAddress(address: string) {
    fireEvent.change(screen.getByLabelText("Your email address"), { target: { value: address } });
  }

  it("signs Gmail in with Google, and connects what it found", async () => {
    native.signInToMailbox.mockResolvedValue(found);
    native.connectSignedInMailbox.mockResolvedValue({});
    renderDialog();
    typeAddress("c@gmail.com");
    expect(
      await screen.findByText(/Gmail signs in with Google in your browser/),
    ).toBeInTheDocument();
    expect(screen.queryByLabelText("App password")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Sign in with Google" }));
    await waitFor(() =>
      expect(native.signInToMailbox).toHaveBeenCalledWith("c@gmail.com", "google"),
    );
    fireEvent.click(await screen.findByRole("button", { name: "Connect and start reading" }));
    await waitFor(() =>
      expect(native.connectSignedInMailbox).toHaveBeenCalledWith("c@gmail.com", true),
    );
  });

  it("still takes an app password for Gmail, and can go back to signing in", async () => {
    renderDialog();
    typeAddress("c@gmail.com");
    fireEvent.click(await screen.findByRole("button", { name: "Use an app password instead" }));
    expect(screen.getByLabelText("App password")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Sign in with Google" })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Sign in with Google instead" }));
    expect(screen.queryByLabelText("App password")).toBeNull();
    expect(screen.getByRole("button", { name: "Sign in with Google" })).toBeEnabled();
  });

  it("asks for an app password, as before, when this copy can't sign in with Google", async () => {
    native.mailSignInAvailable.mockResolvedValue({ microsoft: false, google: false });
    const client = renderDialog();
    typeAddress("c@gmail.com");
    await waitFor(() => expect(client.getQueryState(qk.mailSignIn)?.status).toBe("success"));
    expect(screen.getByLabelText("App password")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Sign in with Google/ })).toBeNull();
    expect(screen.queryByLabelText(/This is a Google account/)).toBeNull();
  });

  it("signs a Google Workspace address in with Google once it's said to be one", async () => {
    native.signInToMailbox.mockResolvedValue(found);
    renderDialog();
    typeAddress("c@formicaria.us");
    expect(await screen.findByLabelText("App password")).toBeInTheDocument();
    expect(screen.queryByLabelText(/This is a Microsoft account/)).toBeNull();
    fireEvent.click(await screen.findByLabelText(/This is a Google account/));
    expect(screen.queryByLabelText("App password")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Sign in with Google" }));
    await waitFor(() =>
      expect(native.signInToMailbox).toHaveBeenCalledWith("c@formicaria.us", "google"),
    );
  });
});

describe("an address at a domain of its own", () => {
  const found: ImapProbe = {
    folders: ["INBOX", "[Gmail]/Sent Mail"],
    sentFolder: "[Gmail]/Sent Mail",
    counts: [["INBOX", 4]],
    warnings: [],
  };

  beforeEach(() => {
    for (const f of Object.values(native)) f.mockReset();
    native.providerState.mockResolvedValue(state("account"));
    native.mailSignInAvailable.mockResolvedValue({ microsoft: false, google: true });
    native.cancelMailSignIn.mockResolvedValue(undefined);
  });

  function leaveAddress(address: string) {
    const field = screen.getByLabelText("Your email address");
    fireEvent.change(field, { target: { value: address } });
    fireEvent.blur(field, { target: { value: address } });
  }

  it("asks who handles its mail, and signs Google Workspace in with Google", async () => {
    native.mailExchangers.mockResolvedValue(["aspmx.l.google.com", "alt1.aspmx.l.google.com"]);
    native.signInToMailbox.mockResolvedValue(found);
    renderDialog();
    leaveAddress("c@Formicaria.us");
    expect(
      await screen.findByText("Mail for formicaria.us is handled by Google Workspace."),
    ).toBeInTheDocument();
    expect(native.mailExchangers).toHaveBeenCalledWith("formicaria.us");
    expect(screen.queryByLabelText("App password")).toBeNull();
    expect(screen.getByRole("button", { name: "Use an app password instead" })).toBeEnabled();
    fireEvent.click(screen.getByRole("button", { name: "Sign in with Google" }));
    await waitFor(() =>
      expect(native.signInToMailbox).toHaveBeenCalledWith("c@Formicaria.us", "google"),
    );
  });

  it("says nothing it didn't find, and asks for the server as before", async () => {
    native.mailExchangers.mockResolvedValue(["mx1.hosting.example.net"]);
    const client = renderDialog();
    leaveAddress("c@formicaria.us");
    await waitFor(() =>
      expect(client.getQueryState(qk.mailExchangers("formicaria.us"))?.status).toBe("success"),
    );
    expect(screen.queryByText(/is handled by/)).toBeNull();
    expect(screen.getByLabelText("App password")).toBeInTheDocument();
    expect(screen.getByText("Server settings")).toBeInTheDocument();
  });

  it("says when Microsoft 365 handles it and this copy can't sign in there", async () => {
    native.mailSignInAvailable.mockResolvedValue({ microsoft: false, google: false });
    native.mailExchangers.mockResolvedValue(["formicaria-us.mail.protection.outlook.com"]);
    renderDialog();
    leaveAddress("c@formicaria.us");
    expect(
      await screen.findByText("Mail for formicaria.us is handled by Microsoft 365."),
    ).toBeInTheDocument();
    expect(screen.getByText(MICROSOFT_UNAVAILABLE)).toBeInTheDocument();
    expect(screen.queryByLabelText("App password")).toBeNull();
  });

  it("checks the server without a password, and says what each server said", async () => {
    native.mailExchangers.mockResolvedValue([]);
    native.reachMailServers.mockResolvedValue({
      reading: {
        host: "imap.formicaria.us",
        port: 993,
        reached: true,
        said: "It answered, encrypted, and tells me as mail arrives (IDLE).",
      },
      sending: null,
    });
    renderDialog();
    leaveAddress("c@formicaria.us");
    fireEvent.click(await screen.findByText("Server settings"));
    fireEvent.change(screen.getByLabelText("IMAP server"), {
      target: { value: "imap.formicaria.us" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Check the server (no password sent)" }));
    expect(
      await screen.findByText(
        "imap.formicaria.us:993 \u2014 It answered, encrypted, and tells me as mail arrives (IDLE).",
      ),
    ).toBeInTheDocument();
    expect(native.reachMailServers).toHaveBeenCalledWith(
      "imap.formicaria.us",
      993,
      "c@formicaria.us",
    );
    // Changing the address forgets it too: the sending server it pairs with
    // is the address's (0.10.0-alpha.93).
    fireEvent.change(screen.getByLabelText("Your email address"), {
      target: { value: "d@formicaria.us" },
    });
    expect(screen.queryByText(/It answered/)).toBeNull();
    // Changing the server forgets what the last one said.
    fireEvent.change(screen.getByLabelText("IMAP server"), {
      target: { value: "mail.formicaria.us" },
    });
    expect(screen.queryByText(/It answered/)).toBeNull();
  });

  it("doesn't show an answer that comes back after the server was changed", async () => {
    native.mailExchangers.mockResolvedValue([]);
    let answer: (v: unknown) => void = () => undefined;
    native.reachMailServers.mockImplementation(
      () =>
        new Promise((resolve) => {
          answer = resolve;
        }),
    );
    renderDialog();
    leaveAddress("c@formicaria.us");
    fireEvent.click(await screen.findByText("Server settings"));
    fireEvent.change(screen.getByLabelText("IMAP server"), {
      target: { value: "imap.formicaria.us" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Check the server (no password sent)" }));
    fireEvent.change(screen.getByLabelText("IMAP server"), {
      target: { value: "mail.formicaria.us" },
    });
    answer({
      reading: {
        host: "imap.formicaria.us",
        port: 993,
        reached: true,
        said: "It answered, encrypted.",
      },
      sending: null,
    });
    await waitFor(() =>
      expect(
        screen.getByRole("button", { name: "Check the server (no password sent)" }),
      ).toBeEnabled(),
    );
    expect(screen.queryByText(/It answered/)).toBeNull();
  });

  it("doesn't ask about a provider it already knows", async () => {
    const client = renderDialog();
    leaveAddress("c@gmail.com");
    await waitFor(() => expect(client.getQueryState(qk.mailSignIn)?.status).toBe("success"));
    expect(native.mailExchangers).not.toHaveBeenCalled();
  });
});
