import { fireEvent, render, screen, within } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { MemoryRouter } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Source } from "@mimic/contracts";
import { SourcesPage } from "../SourcesPage";

// The real hooks run; only the native calls are replaced.
const native = vi.hoisted(() => ({
  sources: vi.fn(),
  deleteSource: vi.fn(),
  startSourceImport: vi.fn(),
  connectors: vi.fn(),
  userIdentity: vi.fn(),
}));
vi.mock("@/lib/ipc", () => ({ ipc: native }));

const source = (over: Partial<Source>): Source => ({
  id: "s1",
  connector: "mimic_json",
  name: "riverastudio-sample.json",
  channel: "email",
  location: "C:\\Users\\sam\\Documents\\exports\\a-very-long-folder-name\\riverastudio-sample.json",
  config: {},
  status: "imported",
  createdAt: "2026-11-08T00:00:00Z",
  lastImportedAt: "2026-11-08T00:00:00Z",
  messageCount: 51,
  lastError: null,
  ...over,
});

function renderIt() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <MemoryRouter>
        <SourcesPage />
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

describe("Your mail", () => {
  beforeEach(() => {
    for (const f of Object.values(native)) f.mockReset();
    native.connectors.mockResolvedValue([]);
    native.userIdentity.mockResolvedValue(null);
  });

  it("lists each source with how it stands, and offers every way in with what it reads", async () => {
    native.sources.mockResolvedValue([source({})]);
    renderIt();
    const list = await screen.findByRole("list");
    const row = within(list).getByRole("listitem");
    expect(within(row).getByText("riverastudio-sample.json")).toBeInTheDocument();
    expect(within(row).getByText("51 messages")).toBeInTheDocument();
    expect(within(row).getByText(/^imported /)).toBeInTheDocument();
    expect(within(row).getByRole("button", { name: "Import again" })).toBeInTheDocument();

    expect(screen.getByRole("heading", { name: "Bring in more" })).toBeInTheDocument();
    for (const name of [
      "Add a file",
      "Connect a mailbox",
      "Connect Slack",
      "Connect Teams",
      "Connect Google Chat",
      "Connect Telegram",
    ]) {
      const way = screen.getByRole("button", { name });
      // Each says what it reads, as its description rather than its name.
      expect(way).toHaveAccessibleDescription(/\S/);
    }
  });

  it("asks before removing a source, says what goes with it, and keeps it on Keep it", async () => {
    native.sources.mockResolvedValue([source({})]);
    renderIt();
    fireEvent.click(await screen.findByRole("button", { name: "Remove" }));
    const ask = screen.getByRole("group", { name: "Remove riverastudio-sample.json" });
    expect(ask).toHaveTextContent("The 51 messages I read from it go too");
    expect(ask).toHaveTextContent("can’t be undone");
    expect(ask).toHaveTextContent("The file itself isn’t touched");
    expect(native.deleteSource).not.toHaveBeenCalled();

    fireEvent.click(within(ask).getByRole("button", { name: "Keep it" }));
    expect(screen.queryByRole("group", { name: /^Remove / })).not.toBeInTheDocument();
    expect(native.deleteSource).not.toHaveBeenCalled();
  });

  it("removes a source only on Remove it", async () => {
    native.sources.mockResolvedValue([source({})]);
    native.deleteSource.mockResolvedValue({});
    renderIt();
    fireEvent.click(await screen.findByRole("button", { name: "Remove" }));
    fireEvent.click(screen.getByRole("button", { name: "Remove it" }));
    // TanStack passes the mutation a second, context argument.
    await vi.waitFor(() => expect(native.deleteSource).toHaveBeenCalled());
    expect(native.deleteSource.mock.calls[0]?.[0]).toBe("s1");
  });

  it("says a connected source's sign-in goes with it, and checks rather than imports it", async () => {
    native.sources.mockResolvedValue([
      source({
        id: "s2",
        connector: "slack",
        name: "Riverastudio",
        channel: "chat",
        location: null,
        config: { team: "T1", user: "sam", lastCheckedAt: "2026-11-08T00:00:00Z" },
      }),
    ]);
    renderIt();
    expect(await screen.findByRole("button", { name: "Check now" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Remove" }));
    const ask = screen.getByRole("group", { name: "Remove Riverastudio" });
    expect(ask).toHaveTextContent("I forget how to sign in to it");
    expect(ask).not.toHaveTextContent("The file itself");
  });

  it("starts from a file when there is nothing yet", async () => {
    native.sources.mockResolvedValue([]);
    renderIt();
    expect(
      await screen.findByRole("heading", { name: "Start with messages you already have" }),
    ).toBeInTheDocument();
    expect(screen.queryByRole("list")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Add a file" })).toHaveClass("way-in--first");
  });
});
