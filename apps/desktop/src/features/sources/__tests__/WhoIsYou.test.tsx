import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { AddressOwner, UserIdentity, WriterName } from "@mimic/contracts";
import { WhoIsYou } from "../WhoIsYou";

// The real hooks run; only the native calls are replaced.
const native = vi.hoisted(() => ({
  userIdentity: vi.fn(),
  previewUserAddress: vi.fn(),
  addUserIdentifier: vi.fn(),
  removeUserIdentifier: vi.fn(),
}));
vi.mock("@/lib/ipc", () => ({ ipc: native }));

const writer = (name: string, messages: number, over: Partial<WriterName> = {}): WriterName => ({
  name,
  kind: "handle",
  value: `whatsapp:${name}`,
  normalized: `whatsapp:${name.toLowerCase()}`,
  messages,
  chatNamedAfter: false,
  ...over,
});

const ada = writer("Ada", 6, { chatNamedAfter: true });
const c = writer("C", 6);
const unsaved = writer("+44 7700 900123", 2, {
  kind: "phone",
  value: "+44 7700 900123",
  normalized: "447700900123",
});

const identity = (extra: { kind: string; value: string; normalizedValue: string }[] = []) =>
  ({
    id: "me",
    displayName: "C",
    identifiers: [
      { id: "e", kind: "email", value: "c@example.com", normalizedValue: "c@example.com" },
      ...extra.map((i, n) => ({ id: `i${n}`, ...i })),
    ],
    createdAt: "",
    updatedAt: "",
  }) satisfies UserIdentity;

const cIsMine = { kind: "handle", value: "whatsapp:C", normalizedValue: "whatsapp:c" };

const owner: AddressOwner = {
  participantId: "p1",
  displayName: "C",
  messages: 6,
  otherAddresses: [],
  relationship: null,
  hasNotes: false,
  preferences: 0,
};

function renderIt(names: WriterName[] = [ada, c, unsaved]) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <WhoIsYou names={names} />
    </QueryClientProvider>,
  );
}

async function button(name: string) {
  const b = await screen.findByRole("button", { name });
  await waitFor(() => expect(b).toBeEnabled());
  return b;
}

describe("which name in a chat is the user's", () => {
  beforeEach(() => {
    for (const f of Object.values(native)) f.mockReset();
    native.addUserIdentifier.mockResolvedValue({
      identity: identity([cIsMine]),
      claimed: { messages: 0, people: 0 },
    });
  });

  it("asks, and adds the address the import gives the name the user picks", async () => {
    native.userIdentity.mockResolvedValueOnce(identity()).mockResolvedValue(identity([cIsMine]));
    native.previewUserAddress.mockResolvedValue({ alreadyYours: false, owner: null });
    renderIt();
    const group = screen.getByRole("group", { name: /Which of these is you\?/ });
    expect(group).toHaveTextContent("I’ll read them as someone else’s");
    fireEvent.click(await button("That’s me: C"));

    await waitFor(() =>
      expect(native.addUserIdentifier).toHaveBeenCalledWith("handle", "whatsapp:C", null),
    );
    expect(native.previewUserAddress).toHaveBeenCalledWith("handle", "whatsapp:C");
    expect(await screen.findByText("I’ll read what C wrote as yours.")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "That’s me: C" })).toBeNull();
  });

  it("adds a writer shown as a number as the phone number the import matches", async () => {
    native.userIdentity.mockResolvedValue(identity());
    native.previewUserAddress.mockResolvedValue({ alreadyYours: false, owner: null });
    renderIt();
    fireEvent.click(await button("That’s me: +44 7700 900123"));
    await waitFor(() =>
      expect(native.addUserIdentifier).toHaveBeenCalledWith("phone", "+44 7700 900123", null),
    );
  });

  it("asks first about the name the chat is named after, which is seldom the user", async () => {
    native.userIdentity.mockResolvedValue(identity());
    native.previewUserAddress.mockResolvedValue({ alreadyYours: false, owner: null });
    renderIt();
    fireEvent.click(await button("That’s me: Ada"));
    const check = screen.getByRole("group", { name: "Is Ada you?" });
    expect(check).toHaveTextContent("This chat is named after Ada");
    fireEvent.click(within(check).getByRole("button", { name: "No" }));
    expect(screen.queryByRole("group", { name: "Is Ada you?" })).toBeNull();
    expect(native.previewUserAddress).not.toHaveBeenCalled();
    expect(native.addUserIdentifier).not.toHaveBeenCalled();
  });

  it("asks first about a second name, and takes back only one said here", async () => {
    const phoneIsMine = {
      kind: "phone",
      value: "+44 7700 900123",
      normalizedValue: "447700900123",
    };
    native.userIdentity
      .mockResolvedValueOnce(identity([cIsMine]))
      .mockResolvedValue(identity([cIsMine, phoneIsMine]));
    native.previewUserAddress.mockResolvedValue({ alreadyYours: false, owner: null });
    native.removeUserIdentifier.mockResolvedValue(identity([cIsMine]));
    renderIt();
    expect(await screen.findByText("I’ll read what C wrote as yours.")).toBeInTheDocument();
    // C was the user's before: taking it back here would not give back what
    // was read under it, so it is not offered.
    expect(screen.getByText("You — already one of your addresses")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Not me: C" })).toBeNull();

    fireEvent.click(await button("That’s me: +44 7700 900123"));
    const check = screen.getByRole("group", { name: "Is +44 7700 900123 you?" });
    expect(check).toHaveTextContent("You’ve said C is you.");
    const yes = within(check).getByRole("button", { name: "Yes, that’s me" });
    expect(yes).toHaveFocus();
    fireEvent.click(yes);
    await waitFor(() =>
      expect(native.addUserIdentifier).toHaveBeenCalledWith("phone", "+44 7700 900123", null),
    );

    fireEvent.click(await button("Not me: +44 7700 900123"));
    await waitFor(() => expect(native.removeUserIdentifier).toHaveBeenCalledWith("i1"));
  });

  it("shows who the name's messages were filed under before they move", async () => {
    native.userIdentity.mockResolvedValueOnce(identity()).mockResolvedValue(identity([cIsMine]));
    native.previewUserAddress.mockResolvedValue({ alreadyYours: false, owner });
    renderIt();
    fireEvent.click(await button("That’s me: C"));
    const question = await screen.findByRole("group", { name: "Is C you?" });
    expect(native.addUserIdentifier).not.toHaveBeenCalled();
    const yes = within(question).getByRole("button", { name: "Yes, that's me" });
    expect(yes).toHaveFocus();
    fireEvent.click(yes);
    await waitFor(() =>
      expect(native.addUserIdentifier).toHaveBeenCalledWith("handle", "whatsapp:C", owner),
    );
    // What moved stays moved, so nothing here offers to take it back.
    expect(await screen.findByText("I’ll read what C wrote as yours.")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Not me: C" })).toBeNull();
  });

  it("shows everyone who wrote when asked, not only the most frequent", async () => {
    native.userIdentity.mockResolvedValue(identity());
    renderIt(Array.from({ length: 10 }, (_, i) => writer(`W${i}`, 10 - i)));
    await screen.findByRole("button", { name: "That’s me: W0" });
    expect(screen.queryByRole("button", { name: "That’s me: W9" })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Show all 10 names" }));
    expect(screen.getByRole("button", { name: "That’s me: W9" })).toBeInTheDocument();
  });
});
