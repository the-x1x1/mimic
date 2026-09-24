import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { MailboxSignInAgain } from "../MailboxPassword";

// The real hooks run; only the native calls are replaced.
const native = vi.hoisted(() => ({
  mailSignInAvailable: vi.fn(),
  signInMailboxAgain: vi.fn(),
  cancelMailSignIn: vi.fn(),
}));
vi.mock("@/lib/ipc", () => ({ ipc: native }));

function renderIt() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <MailboxSignInAgain sourceId="src-1" name="c@outlook.com" />
    </QueryClientProvider>,
  );
}

describe("a Microsoft mailbox can be signed in again without removing it", () => {
  beforeEach(() => {
    for (const f of Object.values(native)) f.mockReset();
    native.mailSignInAvailable.mockResolvedValue(true);
    native.cancelMailSignIn.mockResolvedValue(undefined);
  });

  it("signs in again for that mailbox, and can be stopped while it waits on the browser", async () => {
    let fail: (e: unknown) => void = () => {};
    native.signInMailboxAgain.mockReturnValue(new Promise((_, reject) => (fail = reject)));
    renderIt();
    fireEvent.click(await screen.findByRole("button", { name: "Sign in again" }));
    await waitFor(() => expect(native.signInMailboxAgain).toHaveBeenCalledWith("src-1"));
    expect(await screen.findByText("Finish signing in in your browser.")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Stop" }));
    expect(native.cancelMailSignIn).toHaveBeenCalled();
    fail(Object.assign(new Error("Signing in was stopped."), { code: "canceled" }));
    expect(await screen.findByRole("button", { name: "Sign in again" })).toBeInTheDocument();
    expect(screen.queryByText("Signing in was stopped.")).toBeNull();
  });

  it("says why when the new sign-in doesn't open the mailbox", async () => {
    native.signInMailboxAgain.mockRejectedValue(
      Object.assign(new Error("That sign-in doesn't open c@outlook.com's mailbox."), {
        code: "sign_in",
      }),
    );
    renderIt();
    fireEvent.click(await screen.findByRole("button", { name: "Sign in again" }));
    expect(
      await screen.findByText("That sign-in doesn't open c@outlook.com's mailbox."),
    ).toBeInTheDocument();
  });

  it("offers nothing on a copy of Mimic that can't sign in with Microsoft", async () => {
    native.mailSignInAvailable.mockResolvedValue(false);
    renderIt();
    await waitFor(() => expect(native.mailSignInAvailable).toHaveBeenCalled());
    expect(screen.queryByRole("button", { name: "Sign in again" })).toBeNull();
  });
});
