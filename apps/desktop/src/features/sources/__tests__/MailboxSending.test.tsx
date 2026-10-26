import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { MailboxSending } from "../MailboxSending";

// The real hooks run; only the native calls are replaced.
const native = vi.hoisted(() => ({
  mailboxSending: vi.fn(),
  setMailboxSending: vi.fn(),
  resetMailboxSending: vi.fn(),
  testMailboxSending: vi.fn(),
}));
vi.mock("@/lib/ipc", () => ({ ipc: native }));

function renderIt() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <MailboxSending sourceId="src-1" name="me@example.org" />
    </QueryClientProvider>,
  );
}

const gmail = {
  host: "smtp.gmail.com",
  port: 465,
  security: "tls" as const,
  username: "me@example.org",
  fromAddress: "me@example.org",
};

describe("how a mailbox sends", () => {
  beforeEach(() => {
    for (const f of Object.values(native)) f.mockReset();
  });

  it("says which server replies go out through, and that it is the provider's usual one", async () => {
    native.mailboxSending.mockResolvedValue({ sending: gmail, setByUser: false, keepsSent: true });
    renderIt();
    expect(
      await screen.findByText(
        /Replies go out through smtp.gmail.com as me@example.org, this provider's usual server/,
      ),
    ).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Use the usual server" })).toBeNull();
  });

  it("is set up by hand for a provider it doesn't know, checked before it is kept", async () => {
    native.mailboxSending.mockResolvedValue({ sending: null, setByUser: false, keepsSent: false });
    native.setMailboxSending.mockImplementation(async (_id: string, sending: unknown) => ({
      sending,
      setByUser: true,
      keepsSent: false,
    }));
    renderIt();
    fireEvent.click(await screen.findByRole("button", { name: "Set up sending" }));
    fireEvent.change(screen.getByLabelText("Outgoing server"), {
      target: { value: "mail.example.org" },
    });
    fireEvent.change(screen.getByLabelText("Port"), { target: { value: "587" } });
    fireEvent.change(screen.getByLabelText("Encryption"), { target: { value: "startTls" } });
    fireEvent.click(screen.getByRole("button", { name: "Check and keep" }));
    await waitFor(() =>
      expect(native.setMailboxSending).toHaveBeenCalledWith("src-1", {
        host: "mail.example.org",
        port: 587,
        security: "startTls",
        username: "me@example.org",
        fromAddress: "me@example.org",
      }),
    );
    expect(
      await screen.findByText(/and I put a copy of each in your Sent folder/),
    ).toBeInTheDocument();
    expect(await screen.findByText(/Replies go out through mail.example.org/)).toBeInTheDocument();
  });

  it("keeps the form open with the server's answer when the check fails", async () => {
    native.mailboxSending.mockResolvedValue({ sending: gmail, setByUser: true, keepsSent: true });
    native.setMailboxSending.mockRejectedValue(
      new Error("The mail server turned down the sign-in for sending (535)."),
    );
    renderIt();
    fireEvent.click(await screen.findByRole("button", { name: "Change" }));
    fireEvent.click(screen.getByRole("button", { name: "Check and keep" }));
    expect(await screen.findByText(/turned down the sign-in for sending/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Use the usual server" })).toBeInTheDocument();
  });

  it("can be tried before replies depend on it, signing in and sending nothing", async () => {
    native.mailboxSending.mockResolvedValue({ sending: gmail, setByUser: false, keepsSent: true });
    native.testMailboxSending.mockResolvedValueOnce("smtp.gmail.com");
    renderIt();
    fireEvent.click(await screen.findByRole("button", { name: "Try it (sends nothing)" }));
    expect(
      await screen.findByText(
        "I signed in to smtp.gmail.com and sent nothing: replies can go out from me@example.org.",
      ),
    ).toBeInTheDocument();
    expect(native.testMailboxSending).toHaveBeenCalledWith("src-1");

    native.testMailboxSending.mockRejectedValueOnce(
      new Error("The mail server turned down the sign-in for sending (535)."),
    );
    fireEvent.click(screen.getByRole("button", { name: "Try it (sends nothing)" }));
    expect(
      await screen.findByText("The mail server turned down the sign-in for sending (535)."),
    ).toBeInTheDocument();
  });
});
