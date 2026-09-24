import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ConnectorInfo, UserIdentity, ValidationReport } from "@mimic/contracts";
import { AddSourceDialog } from "../AddSourceDialog";

// The real hooks run; only the native calls are replaced.
const native = vi.hoisted(() => ({
  connectors: vi.fn(),
  pickSourceFile: vi.fn(),
  pickSourceFolder: vi.fn(),
  validateSourceFile: vi.fn(),
  userIdentity: vi.fn(),
  previewUserAddress: vi.fn(),
  addUserIdentifier: vi.fn(),
  removeUserIdentifier: vi.fn(),
  createSource: vi.fn(),
  startSourceImport: vi.fn(),
}));
vi.mock("@/lib/ipc", () => ({ ipc: native }));

const discord: ConnectorInfo = {
  connector: "discord",
  displayName: "Discord data package",
  channel: "chat",
  description: "The package.zip Discord sends when you request all of your data.",
  locationKind: "fileOrFolder",
  extensions: ["zip"],
};

const report: ValidationReport = {
  ok: true,
  blockers: [],
  warnings: [],
  conversations: 2,
  messages: 5,
  frequentIdentifiers: [],
  earliest: null,
  latest: null,
  sentFolder: 0,
  names: [
    {
      name: "C",
      kind: "account_id",
      value: "discord:90001",
      normalized: "discord:90001",
      messages: 5,
      chatNamedAfter: false,
      also: [{ kind: "email", value: "c@example.com", normalized: "c@example.com" }],
    },
  ],
  oneWriter: true,
};

const address = (kind: string, value: string) => ({
  id: `${kind}:${value}`,
  kind,
  value,
  normalizedValue: value.toLowerCase(),
});

const identity = (...identifiers: ReturnType<typeof address>[]) =>
  ({
    id: "me",
    displayName: "C",
    identifiers,
    createdAt: "",
    updatedAt: "",
  }) satisfies UserIdentity;

const folder = "C:\\Users\\c\\Downloads\\package";

function renderDialog() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <AddSourceDialog onClose={() => {}} />
    </QueryClientProvider>,
  );
}

async function chooseDiscordFolder() {
  await screen.findByRole("option", { name: "Discord data package" });
  fireEvent.change(screen.getByLabelText("Format"), { target: { value: "discord" } });
  expect(screen.getByRole("button", { name: "Choose a file" })).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Choose the unzipped folder" }));
  await waitFor(() => expect(native.validateSourceFile).toHaveBeenCalledWith("discord", folder));
  expect(native.pickSourceFile).not.toHaveBeenCalled();
  // A folder was chosen, not a file.
  expect(screen.getByRole("button", { name: "Choose a different folder" })).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Choose a file" })).toBeInTheDocument();
}

describe("adding a Discord package", () => {
  beforeEach(() => {
    for (const f of Object.values(native)) f.mockReset();
    native.connectors.mockResolvedValue([discord]);
    native.pickSourceFolder.mockResolvedValue(folder);
    native.validateSourceFile.mockResolvedValue(report);
    native.previewUserAddress.mockResolvedValue({ alreadyYours: false, owner: null });
    native.createSource.mockResolvedValue({ id: "s1" });
    native.startSourceImport.mockResolvedValue(undefined);
  });

  it("imports it only once the account is the user's", async () => {
    // What is stored changes only when the address is added: the dialog and
    // the question each read it, and may read it again, before that.
    let stored = identity(address("email", "c@work.example"));
    native.userIdentity.mockImplementation(async () => stored);
    native.addUserIdentifier.mockImplementation(async () => {
      stored = identity(address("email", "c@work.example"), address("account_id", "discord:90001"));
      return { identity: stored, claimed: { messages: 0, people: 0 } };
    });
    renderDialog();
    await chooseDiscordFolder();

    const importIt = screen.getByRole("button", { name: "Import" });
    expect(importIt).toBeDisabled();
    expect(importIt).toHaveAccessibleDescription("Say the account is yours before importing it.");

    const yes = await screen.findByRole("button", { name: "That’s me: C" });
    await waitFor(() => expect(yes).toBeEnabled());
    fireEvent.click(yes);
    await waitFor(() =>
      expect(native.addUserIdentifier).toHaveBeenCalledWith("account_id", "discord:90001", null),
    );
    await waitFor(() => expect(importIt).toBeEnabled());
    expect(importIt).not.toHaveAccessibleDescription();

    fireEvent.click(importIt);
    await waitFor(() => expect(native.startSourceImport).toHaveBeenCalled());
    expect(native.createSource.mock.calls[0]?.[0]).toEqual({
      connector: "discord",
      name: "package",
      channel: "chat",
      location: folder,
    });
    expect(native.startSourceImport.mock.calls[0]?.[0]).toBe("s1");
  });

  it("needs nothing more when the account's email is the user's already", async () => {
    native.userIdentity.mockResolvedValue(identity(address("email", "c@example.com")));
    renderDialog();
    await chooseDiscordFolder();
    expect(await screen.findByRole("group", { name: /This is you\./ })).toBeInTheDocument();
    await waitFor(() => expect(screen.getByRole("button", { name: "Import" })).toBeEnabled());
    expect(screen.queryByText("Say the account is yours before importing it.")).toBeNull();
  });
});
