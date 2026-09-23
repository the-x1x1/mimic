import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ParticipantSummary, PeopleView } from "@mimic/contracts";
import { PeoplePage } from "../PeoplePage";

// The real hooks and query client run; only the native call is replaced, so
// loading, failure and the query keys behave as they do in the app.
const native = vi.hoisted(() => ({ people: vi.fn(), setPersonRelationship: vi.fn() }));
vi.mock("@/lib/ipc", () => ({ ipc: native }));

const person = (name: string, automated = false): ParticipantSummary => ({
  participant: {
    id: name,
    displayName: name,
    isSelf: false,
    relationship: null,
    notes: null,
    identifiers: [],
    createdAt: "2026-09-01T00:00:00Z",
    updatedAt: "2026-09-01T00:00:00Z",
  },
  messageCount: 3,
  sentByUser: automated ? 0 : 1,
  conversationCount: 1,
  channels: ["email"],
  firstMessageAt: "2026-09-01T00:00:00Z",
  lastMessageAt: "2026-09-02T00:00:00Z",
  hasRelationshipProfile: false,
  automated,
});

let view: PeopleView;
let senders: Promise<PeopleView>;
let sendersFail = false;

beforeEach(() => {
  native.people.mockReset();
  native.setPersonRelationship.mockReset();
  native.setPersonRelationship.mockResolvedValue(person("Ada").participant);
  view = {
    people: [person("Ada"), person("Grace")],
    peopleTotal: 2,
    automatedSendersTotal: 3,
    automatedSenders: [],
    showingAutomated: false,
  };
  senders = Promise.resolve({
    ...view,
    showingAutomated: true,
    automatedSenders: [person("Brand Weekly", true)],
  });
  sendersFail = false;
  native.people.mockImplementation((_limit: number, automatedLimit: number | null) => {
    if (automatedLimit === null) return Promise.resolve(view);
    return sendersFail ? Promise.reject(new Error("the database is busy")) : senders;
  });
});

function renderPage() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <PeoplePage />
    </QueryClientProvider>,
  );
}

describe("People lists people", () => {
  it("leaves the senders of automated mail out, says how many, and asks for them only when shown", async () => {
    renderPage();
    expect(await screen.findByText("Ada")).toBeInTheDocument();
    expect(
      screen.getByText(/I left out 3 senders whose mail all looks automated/),
    ).toBeInTheDocument();
    expect(screen.queryByText("Brand Weekly")).toBeNull();
    expect(native.people).toHaveBeenCalledTimes(1);
    expect(native.people).toHaveBeenCalledWith(200, null);
  });

  it("says it is looking, then shows the senders, with a way to put one back", async () => {
    let release: (v: PeopleView) => void = () => undefined;
    senders = new Promise((resolve) => {
      release = resolve;
    });
    renderPage();
    fireEvent.click(await screen.findByRole("button", { name: "Show them" }));
    expect(await screen.findByText(/Looking/)).toBeInTheDocument();
    release({ ...view, showingAutomated: true, automatedSenders: [person("Brand Weekly", true)] });
    expect(await screen.findByText("Brand Weekly")).toBeInTheDocument();
    expect(native.people).toHaveBeenLastCalledWith(0, 200);
    expect(screen.getByLabelText("How you know Brand Weekly")).toBeInTheDocument();
    expect(screen.getByText("Ada")).toBeInTheDocument();
  });

  it("keeps the people and the way back when listing the senders fails", async () => {
    sendersFail = true;
    renderPage();
    fireEvent.click(await screen.findByRole("button", { name: "Show them" }));
    expect(
      await screen.findByText(/I couldn’t list them: the database is busy/),
    ).toBeInTheDocument();
    expect(screen.getByText("Ada")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Hide them" })).toBeInTheDocument();
  });

  it("saves a relationship only when it changed", async () => {
    renderPage();
    const input = await screen.findByLabelText("How you know Ada");
    fireEvent.blur(input);
    expect(native.setPersonRelationship).not.toHaveBeenCalled();
    fireEvent.change(input, { target: { value: "friend" } });
    fireEvent.blur(input);
    // A mutation runs its function on the next tick, not inside the event.
    await waitFor(() => expect(native.setPersonRelationship).toHaveBeenCalledWith("Ada", "friend"));
    expect(native.setPersonRelationship).toHaveBeenCalledTimes(1);
  });

  it("says when the list is cut short", async () => {
    view = { ...view, peopleTotal: 250, automatedSendersTotal: 0 };
    renderPage();
    expect(await screen.findByText(/Here are the 2 most recent of 250\./)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Show them" })).toBeNull();
  });
});
