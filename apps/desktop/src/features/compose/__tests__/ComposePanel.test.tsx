import { readFileSync } from "node:fs";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { MemoryRouter, useNavigate } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { GenerationContext, type Draft } from "@mimic/contracts";
import { contractFixture } from "@mimic/test-fixtures";
import { useAnywhere } from "@/state/anywhere";
import { ComposePanel } from "../ComposePanel";

// The real hooks run; only the native calls are replaced.
const native = vi.hoisted(() => ({
  people: vi.fn(),
  providerState: vi.fn(),
  checkProvider: vi.fn(),
  generationContext: vi.fn(),
  generateDraft: vi.fn(),
  resolveDraft: vi.fn(),
  stepAside: vi.fn(),
  pasteBack: vi.fn(),
  settings: vi.fn(),
}));
vi.mock("@/lib/ipc", () => ({ ipc: native }));

const context = GenerationContext.parse(
  JSON.parse(readFileSync(contractFixture("generation_context.json"), "utf8")),
);

const draft = (id: string, text: string): Draft => ({
  id,
  participantId: null,
  conversationId: null,
  channel: "email",
  situationId: null,
  incomingMessage: null,
  intent: "yes, thursday",
  generatedText: text,
  finalText: null,
  provider: "local",
  model: "llama3.2:3b",
  context: {},
  promptHash: "h",
  evidence: {},
  createdAt: "2026-09-23T10:00:00Z",
  resolvedAt: null,
  outcome: null,
  incomingMessageId: null,
  alternativeTo: null,
});

describe("writing something new", () => {
  beforeEach(() => {
    for (const f of Object.values(native)) f.mockReset();
    native.settings.mockResolvedValue({
      "anywhere.pressEnter": false,
      "anywhere.draftAtOnce": false,
    });
    native.people.mockResolvedValue({ people: [], peopleTotal: 0 });
    native.providerState.mockResolvedValue({
      providers: [{ id: "local", displayName: "Local model", local: true }],
      active: "local",
      configuredSecrets: [],
    });
    native.checkProvider.mockResolvedValue(undefined);
    native.generationContext.mockResolvedValue(context);
    native.resolveDraft.mockImplementation(async (id: string, outcome: string) => ({
      ...draft(id, ""),
      outcome,
    }));
  });

  it("records a draft it replaced as passed over, not left waiting", async () => {
    native.generateDraft
      .mockResolvedValueOnce(draft("d1", "yes, thursday works"))
      .mockResolvedValueOnce(draft("d2", "thursday!"));
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(
      <QueryClientProvider client={client}>
        <MemoryRouter>
          <ComposePanel />
        </MemoryRouter>
      </QueryClientProvider>,
    );
    const write = await screen.findByRole("button", { name: "Write a draft" });
    await waitFor(() => expect(write).toBeEnabled());
    fireEvent.click(write);
    expect(await screen.findByDisplayValue("yes, thursday works")).toBeInTheDocument();
    expect(native.resolveDraft).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "Shorter" }));
    expect(await screen.findByDisplayValue("thursday!")).toBeInTheDocument();
    expect(native.resolveDraft).toHaveBeenCalledWith("d1", "regenerated", null);
    expect(native.resolveDraft).toHaveBeenCalledTimes(1);
  });

  it("opens on a message copied from anywhere, as the message being answered", async () => {
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(
      <QueryClientProvider client={client}>
        <MemoryRouter
          initialEntries={[{ pathname: "/write", state: { pasted: "are we still on for 7?" } }]}
        >
          <ComposePanel />
        </MemoryRouter>
      </QueryClientProvider>,
    );
    expect(await screen.findByDisplayValue("are we still on for 7?")).toBeInTheDocument();
    await waitFor(() =>
      expect(native.generationContext).toHaveBeenCalledWith(
        expect.objectContaining({ incomingMessage: "are we still on for 7?" }),
      ),
    );
  });

  describe("answered from anywhere", () => {
    const writeText = vi.fn();

    beforeEach(() => {
      useAnywhere.setState({ copied: null });
      writeText.mockReset();
      writeText.mockResolvedValue(undefined);
      Object.defineProperty(navigator, "clipboard", {
        value: { writeText },
        configurable: true,
      });
      native.stepAside.mockResolvedValue(true);
      native.generateDraft.mockResolvedValue(draft("d1", "yes, see you at 7"));
    });

    it("records a draft for one message as passed over when another is copied in", async () => {
      function CopyAnother() {
        const navigate = useNavigate();
        return (
          <button type="button" onClick={() => navigate("/write", { state: { pasted: "or 8?" } })}>
            Copy another
          </button>
        );
      }
      const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
      render(
        <QueryClientProvider client={client}>
          <MemoryRouter
            initialEntries={[{ pathname: "/write", state: { pasted: "are we still on for 7?" } }]}
          >
            <ComposePanel />
            <CopyAnother />
          </MemoryRouter>
        </QueryClientProvider>,
      );
      const write = await screen.findByRole("button", { name: "Write a draft" });
      await waitFor(() => expect(write).toBeEnabled());
      fireEvent.click(write);
      expect(await screen.findByDisplayValue("yes, see you at 7")).toBeInTheDocument();
      expect(native.resolveDraft).not.toHaveBeenCalled();

      fireEvent.click(screen.getByRole("button", { name: "Copy another" }));
      expect(await screen.findByDisplayValue("or 8?")).toBeInTheDocument();
      await waitFor(() =>
        expect(native.resolveDraft).toHaveBeenCalledWith("d1", "regenerated", null),
      );
      expect(native.resolveDraft).toHaveBeenCalledTimes(1);
      expect(screen.queryByDisplayValue("yes, see you at 7")).not.toBeInTheDocument();
    });

    it("doesn't put a draft for one message under the next one copied in while it was written", async () => {
      native.settings.mockResolvedValue({
        "anywhere.pressEnter": false,
        "anywhere.draftAtOnce": true,
      });
      let finish: (d: Draft) => void = () => undefined;
      native.generateDraft
        .mockImplementationOnce(
          () =>
            new Promise<Draft>((done) => {
              finish = done;
            }),
        )
        .mockResolvedValueOnce(draft("d2", "8 works too"));
      function CopyAnother() {
        const navigate = useNavigate();
        return (
          <button type="button" onClick={() => navigate("/write", { state: { pasted: "or 8?" } })}>
            Copy another
          </button>
        );
      }
      const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
      render(
        <QueryClientProvider client={client}>
          <MemoryRouter
            initialEntries={[{ pathname: "/write", state: { pasted: "are we still on for 7?" } }]}
          >
            <ComposePanel />
            <CopyAnother />
          </MemoryRouter>
        </QueryClientProvider>,
      );
      // The first draft is still being written when the next message comes.
      await waitFor(() => expect(native.generateDraft).toHaveBeenCalledTimes(1));
      fireEvent.click(screen.getByRole("button", { name: "Copy another" }));
      expect(await screen.findByDisplayValue("or 8?")).toBeInTheDocument();
      finish(draft("d1", "yes, see you at 7"));

      // It is put aside, and the next message gets a draft of its own.
      await waitFor(() =>
        expect(native.resolveDraft).toHaveBeenCalledWith("d1", "regenerated", null),
      );
      expect(await screen.findByDisplayValue("8 works too")).toBeInTheDocument();
      expect(screen.queryByDisplayValue("yes, see you at 7")).not.toBeInTheDocument();
      expect(native.generateDraft.mock.calls[1]![0]).toMatchObject({ incomingMessage: "or 8?" });
    });

    async function draftFromAnywhere() {
      const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
      render(
        <QueryClientProvider client={client}>
          <MemoryRouter
            initialEntries={[{ pathname: "/write", state: { pasted: "are we still on for 7?" } }]}
          >
            <ComposePanel />
          </MemoryRouter>
        </QueryClientProvider>,
      );
      const write = await screen.findByRole("button", { name: "Write a draft" });
      await waitFor(() => expect(write).toBeEnabled());
      fireEvent.click(write);
      expect(await screen.findByDisplayValue("yes, see you at 7")).toBeInTheDocument();
    }

    it("copies the reply and steps aside, then asks whether it went", async () => {
      await draftFromAnywhere();
      expect(screen.queryByRole("button", { name: "Copy" })).toBeNull();
      fireEvent.change(screen.getByDisplayValue("yes, see you at 7"), {
        target: { value: "yes! see you at 7" },
      });
      fireEvent.click(screen.getByRole("button", { name: "Copy and go back" }));

      await waitFor(() => expect(native.stepAside).toHaveBeenCalled());
      expect(writeText).toHaveBeenCalledWith("yes! see you at 7");
      expect(native.resolveDraft).not.toHaveBeenCalled();
      expect(await screen.findByText("The reply you copied")).toBeInTheDocument();
      expect(screen.getByText("yes! see you at 7")).toBeInTheDocument();

      fireEvent.click(screen.getByRole("button", { name: "It went as I copied it" }));
      await waitFor(() =>
        expect(native.resolveDraft).toHaveBeenCalledWith("d1", "sent_edited", "yes! see you at 7"),
      );
      await waitFor(() => expect(screen.queryByText("The reply you copied")).toBeNull());
    });

    it("chooses who it's from when the window's title names them", async () => {
      native.people.mockResolvedValue({
        people: [
          { participant: { id: "p1", displayName: "Ada Lovelace", relationship: null } },
          { participant: { id: "p2", displayName: "Bob Stone", relationship: null } },
        ],
        peopleTotal: 2,
      });
      const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
      render(
        <QueryClientProvider client={client}>
          <MemoryRouter
            initialEntries={[
              {
                pathname: "/write",
                state: {
                  pasted: "are we still on for 7?",
                  windowTitle: "Ada Lovelace | Messenger",
                },
              },
            ]}
          >
            <ComposePanel />
          </MemoryRouter>
        </QueryClientProvider>,
      );
      expect(
        await screen.findByText("From the window you copied it in, this looks like Ada Lovelace."),
      ).toBeInTheDocument();
      expect(screen.getByLabelText("Recipient")).toHaveValue("p1");
      fireEvent.change(screen.getByLabelText("Recipient"), { target: { value: "p2" } });
      expect(screen.queryByText(/this looks like Ada Lovelace/)).toBeNull();
    });

    it("doesn't carry whom one window named over to the next message", async () => {
      native.people.mockResolvedValue({
        people: [
          { participant: { id: "p1", displayName: "Ada Lovelace", relationship: null } },
          { participant: { id: "p2", displayName: "Bob Stone", relationship: null } },
        ],
        peopleTotal: 2,
      });
      function CopyFromWhatsApp() {
        const navigate = useNavigate();
        return (
          <button
            type="button"
            onClick={() =>
              navigate("/write", { state: { pasted: "or 8?", windowTitle: "WhatsApp" } })
            }
          >
            Copy from WhatsApp
          </button>
        );
      }
      const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
      render(
        <QueryClientProvider client={client}>
          <MemoryRouter
            initialEntries={[
              {
                pathname: "/write",
                state: {
                  pasted: "are we still on for 7?",
                  windowTitle: "Ada Lovelace | Messenger",
                },
              },
            ]}
          >
            <ComposePanel />
            <CopyFromWhatsApp />
          </MemoryRouter>
        </QueryClientProvider>,
      );
      await waitFor(() => expect(screen.getByLabelText("Recipient")).toHaveValue("p1"));
      fireEvent.click(screen.getByRole("button", { name: "Copy from WhatsApp" }));
      expect(await screen.findByDisplayValue("or 8?")).toBeInTheDocument();
      expect(screen.getByLabelText("Recipient")).toHaveValue("");
      expect(screen.queryByText(/this looks like Ada/)).toBeNull();
      // What WhatsApp is, its title still says.
      expect(
        screen.getByText("From the window you copied it in, this looks like a chat."),
      ).toBeInTheDocument();
    });

    it("answers a chat as a chat, by the window's title, and lets go of that after", async () => {
      native.settings.mockResolvedValue({
        "anywhere.pressEnter": false,
        "anywhere.draftAtOnce": true,
      });
      function CopyFromNotepad() {
        const navigate = useNavigate();
        return (
          <button
            type="button"
            onClick={() =>
              navigate("/write", { state: { pasted: "or 8?", windowTitle: "Untitled - Notepad" } })
            }
          >
            Copy from Notepad
          </button>
        );
      }
      const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
      render(
        <QueryClientProvider client={client}>
          <MemoryRouter
            initialEntries={[
              {
                pathname: "/write",
                state: {
                  pasted: "are we still on for 7?",
                  windowTitle: "Ada Lovelace | Messenger",
                },
              },
            ]}
          >
            <ComposePanel />
            <CopyFromNotepad />
          </MemoryRouter>
        </QueryClientProvider>,
      );
      expect(
        await screen.findByText("From the window you copied it in, this looks like a chat."),
      ).toBeInTheDocument();
      expect(screen.getByLabelText("Channel")).toHaveValue("chat");
      // The draft written at once is written for a chat.
      await waitFor(() => expect(native.generateDraft).toHaveBeenCalledTimes(1));
      expect(native.generateDraft.mock.calls[0]![0]).toMatchObject({ channel: "chat" });

      fireEvent.click(screen.getByRole("button", { name: "Copy from Notepad" }));
      expect(await screen.findByDisplayValue("or 8?")).toBeInTheDocument();
      expect(screen.getByLabelText("Channel")).toHaveValue("email");
      expect(screen.queryByText(/this looks like a chat/)).toBeNull();
    });

    it("pastes the reply into the window the message came from, then asks whether it went", async () => {
      native.pasteBack.mockResolvedValue(null);
      await draftFromAnywhere();
      fireEvent.click(screen.getByRole("button", { name: "Paste it there" }));
      await waitFor(() => expect(native.pasteBack).toHaveBeenCalledWith("yes, see you at 7"));
      expect(native.stepAside).not.toHaveBeenCalled();
      expect(native.resolveDraft).not.toHaveBeenCalled();
      expect(await screen.findByText("The reply you copied")).toBeInTheDocument();
      fireEvent.click(screen.getByRole("button", { name: "It went as I copied it" }));
      await waitFor(() =>
        expect(native.resolveDraft).toHaveBeenCalledWith(
          "d1",
          "sent_unedited",
          "yes, see you at 7",
        ),
      );
    });

    it("pastes and presses Enter only when that is turned on, and still asks whether it went", async () => {
      await draftFromAnywhere();
      await waitFor(() => expect(native.settings).toHaveBeenCalled());
      expect(screen.getByRole("button", { name: "Paste it there" })).toBeInTheDocument();
      expect(screen.queryByRole("button", { name: "Paste it there and send" })).toBeNull();
      cleanup();

      native.settings.mockResolvedValue({
        "anywhere.pressEnter": true,
        "anywhere.draftAtOnce": false,
      });
      native.pasteBack.mockResolvedValue(null);
      await draftFromAnywhere();
      fireEvent.click(await screen.findByRole("button", { name: "Paste it there and send" }));
      await waitFor(() => expect(native.pasteBack).toHaveBeenCalledWith("yes, see you at 7", true));
      expect(native.resolveDraft).not.toHaveBeenCalled();
      expect(await screen.findByText("The reply you copied")).toBeInTheDocument();
    });

    it("writes a draft at once when opened from anywhere, and puts the keyboard on pasting it", async () => {
      native.settings.mockResolvedValue({
        "anywhere.pressEnter": false,
        "anywhere.draftAtOnce": true,
      });
      const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
      render(
        <QueryClientProvider client={client}>
          <MemoryRouter
            initialEntries={[{ pathname: "/write", state: { pasted: "are we still on for 7?" } }]}
          >
            <ComposePanel />
          </MemoryRouter>
        </QueryClientProvider>,
      );
      // Nothing pressed: the draft is written once the model is ready.
      expect(await screen.findByDisplayValue("yes, see you at 7")).toBeInTheDocument();
      expect(native.generateDraft).toHaveBeenCalledTimes(1);
      expect(native.generateDraft.mock.calls[0]![0]).toMatchObject({
        incomingMessage: "are we still on for 7?",
      });
      await waitFor(() =>
        expect(screen.getByRole("button", { name: "Paste it there" })).toHaveFocus(),
      );
    });

    it("records a reply that didn't go, and nothing for one changed after copying", async () => {
      await draftFromAnywhere();
      fireEvent.click(screen.getByRole("button", { name: "Copy and go back" }));
      fireEvent.click(await screen.findByRole("button", { name: "It didn’t go" }));
      await waitFor(() =>
        expect(native.resolveDraft).toHaveBeenCalledWith("d1", "discarded", null),
      );

      useAnywhere.setState({
        copied: { draftId: "d2", text: "sure", generatedText: "sure" },
      });
      fireEvent.click(await screen.findByRole("button", { name: "I changed it first" }));
      await waitFor(() => expect(screen.queryByText("The reply you copied")).toBeNull());
      expect(native.resolveDraft).toHaveBeenCalledTimes(1);
    });
  });
});
