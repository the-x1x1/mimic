import { fireEvent, render, screen, within } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { MailboxFolders } from "../MailboxFolders";

// The real hooks run; only the native calls are replaced.
const native = vi.hoisted(() => ({
  mailboxFolders: vi.fn(),
  setMailboxFolders: vi.fn(),
}));
vi.mock("@/lib/ipc", () => ({ ipc: native }));

function renderIt() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <MailboxFolders sourceId="src-1" name="me@example.org" />
    </QueryClientProvider>,
  );
}

const offered = {
  folders: [
    { name: "Receipts", shown: "Receipts", chosen: true },
    { name: "&AMk-tudes", shown: "Études", chosen: false },
  ],
  gone: ["Old rules"],
  unopened: ["Receipts"],
  most: 10,
};

describe("other folders a mailbox is read in", () => {
  beforeEach(() => {
    for (const f of Object.values(native)) f.mockReset();
  });

  it("asks the server only once shown, and keeps what is ticked", async () => {
    native.mailboxFolders.mockResolvedValue(offered);
    native.setMailboxFolders.mockImplementation(async (_id: string, folders: string[]) => ({
      ...offered,
      folders: offered.folders.map((f) => ({ ...f, chosen: folders.includes(f.name) })),
      gone: [],
      unopened: [],
    }));
    renderIt();
    expect(native.mailboxFolders).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Other folders to read" }));
    const form = await screen.findByRole("form", {
      name: "Other folders to read in me@example.org",
    });
    expect(within(form).getByText(/I never reply to it by myself/)).toBeInTheDocument();
    expect(within(form).getByText(/no longer in me@example.org/)).toHaveTextContent("Old rules");
    expect(within(form).getByText(/couldn.t open Receipts/)).toBeInTheDocument();
    expect(within(form).getByLabelText("Receipts")).toBeChecked();
    fireEvent.click(within(form).getByLabelText("Études"));
    fireEvent.click(within(form).getByRole("button", { name: "Read these" }));
    expect(await screen.findByText(/I'll read those 2 folders too/)).toBeInTheDocument();
    expect(native.setMailboxFolders).toHaveBeenCalledWith("src-1", ["Receipts", "&AMk-tudes"]);
  });

  it("says what the server said when it can't look", async () => {
    native.mailboxFolders.mockRejectedValue(new Error("I couldn't sign in to imap.example.org."));
    renderIt();
    fireEvent.click(screen.getByRole("button", { name: "Other folders to read" }));
    expect(await screen.findByText("I couldn't sign in to imap.example.org.")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Never mind" }));
    expect(screen.getByRole("button", { name: "Other folders to read" })).toBeInTheDocument();
  });
});
