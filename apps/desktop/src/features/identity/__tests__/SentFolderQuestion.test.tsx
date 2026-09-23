import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { AddressOwner, SentFolderPerson } from "@mimic/contracts";
import { SentFolderNotice, SentFolderQuestion } from "../AddAddressForm";

// The real hooks run; only the native calls are replaced.
const native = vi.hoisted(() => ({
  addUserIdentifier: vi.fn(),
  keepPersonApart: vi.fn(),
  sentFolderPeople: vi.fn(),
}));
vi.mock("@/lib/ipc", () => ({ ipc: native }));

const owner: AddressOwner = {
  participantId: "p9",
  displayName: "C at work",
  messages: 14,
  otherAddresses: ["c@old-work.example"],
  relationship: null,
  hasNotes: false,
  preferences: 0,
};

const found = (o: AddressOwner, sent = 12): SentFolderPerson => ({
  kind: "email",
  address: "c@work.example",
  sent,
  owner: o,
});

const added = {
  identity: { id: "me", displayName: "C", identifiers: [], createdAt: "", updatedAt: "" },
  claimed: { messages: 14, people: 1 },
};

const newClient = () => new QueryClient({ defaultOptions: { queries: { retry: false } } });

function withClient(node: ReactNode, client = newClient()) {
  return <QueryClientProvider client={client}>{node}</QueryClientProvider>;
}

describe("someone whose mail was in the user's Sent folder", () => {
  beforeEach(() => {
    for (const f of Object.values(native)) f.mockReset();
  });

  it("says what was found and what it costs, and asks only when opened", async () => {
    native.addUserIdentifier.mockResolvedValue(added);
    render(withClient(<SentFolderQuestion person={found(owner)} />));
    expect(
      screen.getByText(
        "12 of the 14 messages I have from C at work (c@work.example) were in your Sent folder. If that's you, I'm counting what you wrote as someone else's.",
      ),
    ).toBeInTheDocument();
    expect(screen.queryByRole("group")).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "Is that you?" }));
    // Why a yes isn't a given, before the question is answered.
    expect(screen.getByText(/unless someone else sends for you/)).toBeInTheDocument();
    const question = screen.getByRole("group", { name: "Is C at work you?" });
    // Every message filed under them moves, not only the ones in the Sent folder.
    expect(question).toHaveTextContent("the 14 messages filed under them become yours");
    expect(question).toHaveTextContent(
      "Their other address, c@old-work.example, becomes yours too",
    );

    fireEvent.click(screen.getByRole("button", { name: "Yes, that's me" }));
    await waitFor(() =>
      expect(native.addUserIdentifier).toHaveBeenCalledWith("email", "c@work.example", owner),
    );
    await waitFor(() => expect(screen.queryByRole("group")).toBeNull());
    expect(screen.queryByRole("button", { name: "Is that you?" })).toBeNull();
    expect(native.keepPersonApart).not.toHaveBeenCalled();
  });

  it("keeps a no", async () => {
    native.keepPersonApart.mockResolvedValue(undefined);
    render(withClient(<SentFolderQuestion person={found(owner)} open />));
    fireEvent.click(screen.getByRole("button", { name: "No" }));
    await waitFor(() => expect(native.keepPersonApart).toHaveBeenCalledWith("p9"));
    await waitFor(() => expect(screen.queryByRole("group")).toBeNull());
    expect(native.addUserIdentifier).not.toHaveBeenCalled();
  });

  it("sends what the user read, and shows a change before they can agree to it", async () => {
    native.addUserIdentifier.mockResolvedValue(added);
    const client = newClient();
    const { rerender } = render(
      withClient(<SentFolderQuestion person={found(owner)} open />, client),
    );
    // A check filed another message under them while the question was open.
    const later = { ...owner, messages: 15 };
    rerender(withClient(<SentFolderQuestion person={found(later, 13)} open />, client));
    expect(await screen.findByText(/That changed since I asked/)).toBeInTheDocument();
    expect(screen.getByRole("group", { name: "Is C at work you?" })).toHaveTextContent(
      "the 15 messages filed under them",
    );
    fireEvent.click(screen.getByRole("button", { name: "Yes, that's me" }));
    await waitFor(() =>
      expect(native.addUserIdentifier).toHaveBeenCalledWith("email", "c@work.example", later),
    );
  });

  it("asks again, as it is now, when the yes was to something that has changed", async () => {
    const later = { ...owner, messages: 15 };
    // Read again after the refusal, someone else now comes first.
    const pat = {
      ...found({ ...owner, participantId: "p7", displayName: "Pat", messages: 1 }, 1),
      address: "pat@example.com",
    };
    native.sentFolderPeople
      .mockResolvedValueOnce([found(owner)])
      .mockResolvedValue([pat, found(later, 13)]);
    native.addUserIdentifier
      .mockRejectedValueOnce(Object.assign(new Error("changed"), { code: "confirm" }))
      .mockResolvedValueOnce(added);
    render(withClient(<SentFolderNotice open />));
    await screen.findByRole("group", { name: "Is C at work you?" });
    fireEvent.click(screen.getByRole("button", { name: "Yes, that's me" }));
    expect(await screen.findByText(/That changed since I asked/)).toBeInTheDocument();
    expect(screen.getByRole("group", { name: "Is C at work you?" })).toHaveTextContent(
      "the 15 messages filed under them",
    );
    expect(screen.queryByText("changed")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Yes, that's me" }));
    await waitFor(() =>
      expect(native.addUserIdentifier).toHaveBeenLastCalledWith("email", "c@work.example", later),
    );
  });

  it("puts one person at a time on the home screen, the likeliest first", async () => {
    const pat = {
      ...found({ ...owner, participantId: "p7", displayName: "Pat", otherAddresses: [] }, 2),
      address: "pat@example.com",
    };
    native.sentFolderPeople.mockResolvedValueOnce([found(owner), pat]).mockResolvedValue([pat]);
    native.keepPersonApart.mockResolvedValue(undefined);
    render(withClient(<SentFolderNotice />));
    expect(await screen.findByText(/from C at work \(c@work\.example\)/)).toBeInTheDocument();
    expect(screen.queryByText(/pat@example\.com/)).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "Is that you?" }));
    fireEvent.click(screen.getByRole("button", { name: "No" }));
    expect(
      await screen.findByText(/^2 of the 14 messages I have from Pat \(pat@example\.com\)/),
    ).toBeInTheDocument();
    expect(screen.queryByRole("group")).toBeNull();
  });
});
