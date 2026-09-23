import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type {
  AddressAdded,
  AddressPreview,
  SentFolderPerson,
  Source,
  UserIdentity,
} from "@mimic/contracts";
import { ImportStep } from "../OnboardingFlow";

// The real hooks run; only the native calls are replaced.
const native = vi.hoisted(() => ({
  sources: vi.fn(),
  userIdentity: vi.fn(),
  previewUserAddress: vi.fn(),
  addUserIdentifier: vi.fn(),
  keepPersonApart: vi.fn(),
  sentFolderPeople: vi.fn(),
  startSourceImport: vi.fn(),
  deleteSource: vi.fn(),
}));
vi.mock("@/lib/ipc", () => ({ ipc: native }));

const identity: UserIdentity = {
  id: "me",
  displayName: "C",
  identifiers: [
    { id: "i1", kind: "email", value: "c@example.com", normalizedValue: "c@example.com" },
  ],
  createdAt: "2026-09-22T00:00:00Z",
  updatedAt: "2026-09-22T00:00:00Z",
};

// Read, and nothing in it was the user's: the step's dead end before 0.7.0.
const readButNoneOfYours: Source = {
  id: "s1",
  connector: "mbox",
  name: "export.mbox",
  channel: "email",
  location: "C:/export.mbox",
  config: {},
  status: "imported",
  createdAt: "2026-09-22T00:00:00Z",
  lastImportedAt: "2026-09-22T00:00:00Z",
  messageCount: 40,
  lastError: null,
};

const added = (messages: number): AddressAdded => ({
  identity,
  claimed: { messages, people: messages > 0 ? 1 : 0 },
});

// What the user wrote from their work address was filed under "C (work)".
const filedUnderTheAlias: AddressPreview = {
  alreadyYours: false,
  owner: {
    participantId: "p9",
    displayName: "C (work)",
    messages: 12,
    otherAddresses: [],
    relationship: null,
    hasNotes: false,
    preferences: 0,
  },
};

function renderStep() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <ImportStep />
    </QueryClientProvider>,
  );
}

function addAddress(value: string) {
  fireEvent.change(screen.getByLabelText("Another address of yours"), { target: { value } });
  fireEvent.click(screen.getByRole("button", { name: "Add it" }));
}

describe("when nothing read was the user's, adding the address is the fix", () => {
  beforeEach(() => {
    for (const f of Object.values(native)) f.mockReset();
    native.sources.mockResolvedValue([readButNoneOfYours]);
    native.userIdentity.mockResolvedValue(identity);
    native.sentFolderPeople.mockResolvedValue([]);
  });

  it("asks first about the address the Sent folder names, and adds it on a yes", async () => {
    const found: SentFolderPerson = {
      kind: "email",
      address: "c@work.example",
      sent: 12,
      owner: filedUnderTheAlias.owner!,
    };
    native.sentFolderPeople.mockResolvedValueOnce([found]).mockResolvedValue([]);
    native.addUserIdentifier.mockResolvedValue(added(12));
    renderStep();
    const question = await screen.findByRole("group", { name: "Is C (work) you?" });
    expect(
      screen.getByText(
        /^All 12 messages I have from C \(work\) \(c@work\.example\) were in your Sent folder\. Mail there is usually yours/,
      ),
    ).toBeInTheDocument();
    expect(question).toHaveTextContent("the 12 messages filed under them become yours");
    expect(native.previewUserAddress).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "Yes, that's me" }));
    await waitFor(() =>
      expect(native.addUserIdentifier).toHaveBeenCalledWith(
        "email",
        "c@work.example",
        filedUnderTheAlias.owner,
      ),
    );
    await waitFor(() => expect(screen.queryByRole("group")).toBeNull());
  });

  it("keeps a no about someone the Sent folder names, and doesn't ask again", async () => {
    const found: SentFolderPerson = {
      kind: "email",
      address: "pat@example.com",
      sent: 3,
      owner: { ...filedUnderTheAlias.owner!, participantId: "p7", displayName: "Pat" },
    };
    native.sentFolderPeople.mockResolvedValueOnce([found]).mockResolvedValue([]);
    native.keepPersonApart.mockResolvedValue(undefined);
    renderStep();
    await screen.findByRole("group", { name: "Is Pat you?" });
    fireEvent.click(screen.getByRole("button", { name: "No" }));
    await waitFor(() => expect(native.keepPersonApart).toHaveBeenCalledWith("p7"));
    await waitFor(() => expect(screen.queryByRole("group")).toBeNull());
    expect(native.addUserIdentifier).not.toHaveBeenCalled();
  });

  it("asks whether the person it is filed under is the user, and moves it only on a yes", async () => {
    native.previewUserAddress.mockResolvedValue(filedUnderTheAlias);
    native.addUserIdentifier.mockResolvedValue(added(12));
    renderStep();
    expect(await screen.findByText("Nothing I read was written by you")).toBeInTheDocument();
    addAddress("c@work.example");

    const question = await screen.findByRole("group", { name: "Is C (work) you?" });
    expect(question).toHaveTextContent(
      "If C (work) is you, the 12 messages filed under them become yours",
    );
    expect(native.previewUserAddress).toHaveBeenCalledWith("email", "c@work.example");
    expect(native.addUserIdentifier).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "Yes, that's me" }));
    await waitFor(() => expect(screen.getByLabelText("Another address of yours")).toHaveValue(""));
    expect(native.addUserIdentifier).toHaveBeenCalledWith(
      "email",
      "c@work.example",
      filedUnderTheAlias.owner,
    );
    expect(screen.queryByRole("group", { name: "Is C (work) you?" })).toBeNull();
    // What was read is moved over; nothing is imported again.
    expect(native.startSourceImport).not.toHaveBeenCalled();
    expect(screen.queryByText(/either/)).toBeNull();
  });

  it("changes nothing when the answer is no", async () => {
    native.previewUserAddress.mockResolvedValue(filedUnderTheAlias);
    renderStep();
    await screen.findByText("Nothing I read was written by you");
    addAddress("c@work.example");
    await screen.findByRole("group", { name: "Is C (work) you?" });
    fireEvent.click(screen.getByRole("button", { name: "No" }));
    await waitFor(() => expect(screen.queryByRole("group")).toBeNull());
    expect(native.addUserIdentifier).not.toHaveBeenCalled();
    // Not the user's address, so there is nothing to remember about it.
    expect(native.keepPersonApart).not.toHaveBeenCalled();
  });

  it("adds an address nothing was filed under straight away, and says nothing came from it", async () => {
    native.previewUserAddress.mockResolvedValue({ alreadyYours: false, owner: null });
    native.addUserIdentifier.mockResolvedValue(added(0));
    renderStep();
    await screen.findByText("Nothing I read was written by you");
    addAddress("typo@example.com");
    expect(
      await screen.findByText(/Nothing I.ve read came from typo@example.com either\./),
    ).toBeInTheDocument();
    expect(native.addUserIdentifier).toHaveBeenCalledWith("email", "typo@example.com", null);
  });
  it("asks again, as it is now, when what was shown changed before the yes", async () => {
    const now = { ...filedUnderTheAlias.owner!, messages: 13 };
    native.previewUserAddress
      .mockResolvedValueOnce(filedUnderTheAlias)
      .mockResolvedValueOnce({ alreadyYours: false, owner: now });
    native.addUserIdentifier
      .mockRejectedValueOnce(Object.assign(new Error("changed"), { code: "confirm" }))
      .mockResolvedValueOnce(added(13));
    renderStep();
    await screen.findByText("Nothing I read was written by you");
    addAddress("c@work.example");
    await screen.findByRole("group", { name: "Is C (work) you?" });
    fireEvent.click(screen.getByRole("button", { name: "Yes, that's me" }));

    const again = await screen.findByText(/That changed since I asked/);
    expect(again.closest("[role=group]")).toHaveTextContent("the 13 messages filed under them");
    expect(screen.queryByText("changed")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Yes, that's me" }));
    await waitFor(() =>
      expect(native.addUserIdentifier).toHaveBeenLastCalledWith("email", "c@work.example", now),
    );
  });

  it("says so when the address is already one of the user's", async () => {
    native.previewUserAddress.mockResolvedValue({ alreadyYours: true, owner: null });
    renderStep();
    await screen.findByText("Nothing I read was written by you");
    addAddress("c@example.com");
    expect(await screen.findByText("c@example.com is already one of yours.")).toBeInTheDocument();
    expect(native.addUserIdentifier).not.toHaveBeenCalled();
  });
  it("remembers a no about someone under an address that is already the user's", async () => {
    native.previewUserAddress.mockResolvedValue({ ...filedUnderTheAlias, alreadyYours: true });
    native.keepPersonApart.mockResolvedValue(undefined);
    renderStep();
    await screen.findByText("Nothing I read was written by you");
    addAddress("c@work.example");
    await screen.findByRole("group", { name: "Is C (work) you?" });
    fireEvent.click(screen.getByRole("button", { name: "No" }));
    await waitFor(() => expect(native.keepPersonApart).toHaveBeenCalledWith("p9"));
    await waitFor(() => expect(screen.queryByRole("group")).toBeNull());
    expect(native.addUserIdentifier).not.toHaveBeenCalled();
  });
});
