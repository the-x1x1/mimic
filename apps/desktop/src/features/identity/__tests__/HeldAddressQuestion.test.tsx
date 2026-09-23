import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { AddressOwner, HeldAddress } from "@mimic/contracts";
import { HeldAddressQuestion } from "../AddAddressForm";

const native = vi.hoisted(() => ({
  claimHeldAddress: vi.fn(),
  keepPersonApart: vi.fn(),
}));
vi.mock("@/lib/ipc", () => ({ ipc: native }));

const owner: AddressOwner = {
  participantId: "p9",
  displayName: "C (work)",
  messages: 12,
  otherAddresses: [],
  relationship: "me at work",
  hasNotes: false,
  preferences: 0,
};

const held = (o: AddressOwner, keptApart = false): HeldAddress => ({
  identifier: {
    id: "i2",
    kind: "email",
    value: "c@work.example",
    normalizedValue: "c@work.example",
  },
  owner: o,
  keptApart,
});

const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });

function withClient(node: ReactNode) {
  return <QueryClientProvider client={client}>{node}</QueryClientProvider>;
}

describe("an address of the user's still filed under someone", () => {
  beforeEach(() => {
    for (const f of Object.values(native)) f.mockReset();
  });

  it("says whose it is, and a no is remembered", async () => {
    native.keepPersonApart.mockResolvedValue(undefined);
    render(withClient(<HeldAddressQuestion held={held(owner)} />));
    expect(
      screen.getByText("I still have it down as C (work)'s, with 12 messages filed under them."),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Is that you?" }));
    expect(screen.getByRole("group", { name: "Is C (work) you?" })).toHaveTextContent(
      'what they are to you ("me at work")',
    );
    fireEvent.click(screen.getByRole("button", { name: "No" }));
    await waitFor(() => expect(native.keepPersonApart).toHaveBeenCalledWith("p9"));
    await waitFor(() => expect(screen.queryByRole("group")).toBeNull());
    expect(native.claimHeldAddress).not.toHaveBeenCalled();
  });

  it("says so when the user already said no", () => {
    render(withClient(<HeldAddressQuestion held={held(owner, true)} />));
    expect(screen.getByText(/You said C \(work\) isn't you/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Is that you after all?" })).toBeInTheDocument();
  });

  it("sends what the user read, and shows a change before they can agree to it", async () => {
    native.claimHeldAddress.mockResolvedValue({
      identity: { id: "me", displayName: "C", identifiers: [], createdAt: "", updatedAt: "" },
      claimed: { messages: 13, people: 1 },
    });
    const { rerender } = render(withClient(<HeldAddressQuestion held={held(owner)} />));
    fireEvent.click(screen.getByRole("button", { name: "Is that you?" }));

    // A check filed another message under them while the question was open.
    const later = { ...owner, messages: 13 };
    rerender(withClient(<HeldAddressQuestion held={held(later)} />));
    expect(await screen.findByText(/That changed since I asked/)).toBeInTheDocument();
    expect(screen.getByRole("group", { name: "Is C (work) you?" })).toHaveTextContent(
      "the 13 messages filed under them",
    );
    fireEvent.click(screen.getByRole("button", { name: "Yes, that's me" }));
    await waitFor(() => expect(native.claimHeldAddress).toHaveBeenCalledWith("i2", later));
  });
  it("takes a no about someone already folded in meanwhile as done", async () => {
    native.keepPersonApart.mockRejectedValue(
      Object.assign(new Error("not found: p9"), { code: "not_found" }),
    );
    render(withClient(<HeldAddressQuestion held={held(owner)} />));
    fireEvent.click(screen.getByRole("button", { name: "Is that you?" }));
    fireEvent.click(screen.getByRole("button", { name: "No" }));
    await waitFor(() => expect(screen.queryByRole("group")).toBeNull());
    expect(screen.queryByText(/not found/)).toBeNull();
  });
});
