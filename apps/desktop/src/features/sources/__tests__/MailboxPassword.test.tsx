import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { MailboxPassword } from "../MailboxPassword";

// The real hook runs; only the native call is replaced.
const native = vi.hoisted(() => ({ setMailboxPassword: vi.fn() }));
vi.mock("@/lib/ipc", () => ({ ipc: native }));

function renderIt() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <MailboxPassword sourceId="src-1" name="me@example.com" />
    </QueryClientProvider>,
  );
}

function typeAndSave(password: string) {
  fireEvent.click(screen.getByRole("button", { name: "New password" }));
  fireEvent.change(screen.getByLabelText("New app password for me@example.com"), {
    target: { value: password },
  });
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
}

describe("a mailbox's password can be given again without removing it", () => {
  // A block, not an expression: a function returned from beforeEach is run
  // after the test as its cleanup, and mockReset() returns the mock.
  beforeEach(() => {
    native.setMailboxPassword.mockReset();
  });

  it("saves it for that mailbox and closes", async () => {
    native.setMailboxPassword.mockResolvedValue({});
    renderIt();
    typeAndSave("abcd efgh ijkl mnop");
    await waitFor(() =>
      expect(native.setMailboxPassword).toHaveBeenCalledWith("src-1", "abcd efgh ijkl mnop"),
    );
    expect(await screen.findByRole("button", { name: "New password" })).toBeInTheDocument();
  });

  it("says why when the login fails, and keeps what was typed", async () => {
    native.setMailboxPassword.mockRejectedValue(
      new Error("The server turned down that username and password."),
    );
    renderIt();
    typeAndSave("wrong");
    expect(
      await screen.findByText("The server turned down that username and password."),
    ).toBeInTheDocument();
    expect(screen.getByLabelText("New app password for me@example.com")).toHaveValue("wrong");
  });
});
