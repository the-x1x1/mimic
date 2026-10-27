import { readFileSync } from "node:fs";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
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
  writeAnotherDraft: vi.fn(),
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

    fireEvent.click(screen.getByRole("button", { name: "Regenerate" }));
    expect(await screen.findByDisplayValue("thursday!")).toBeInTheDocument();
    expect(native.resolveDraft).toHaveBeenCalledWith("d1", "regenerated", null);
    expect(native.resolveDraft).toHaveBeenCalledTimes(1);
  });

  describe("other ways of saying it, beside the draft", () => {
    const shorter = (): Draft => ({
      ...draft("d2", "thursday!"),
      alternativeTo: "d1",
      context: { adjustment: "shorter" },
    });

    async function writeTwoWays() {
      native.generateDraft.mockResolvedValueOnce(draft("d1", "yes, thursday works"));
      native.writeAnotherDraft.mockResolvedValueOnce(shorter());
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
      const ways = screen.getByRole("group", { name: "Write it another way" });
      fireEvent.click(within(ways).getByRole("button", { name: "Shorter" }));
      expect(await screen.findByDisplayValue("thursday!")).toBeInTheDocument();
      expect(native.writeAnotherDraft).toHaveBeenCalledWith("d1", "shorter");
    }

    it("writes it shorter beside the first, puts nothing aside, and offers only what isn't there", async () => {
      await writeTwoWays();
      expect(native.resolveDraft).not.toHaveBeenCalled();
      expect(screen.getByRole("radio", { name: "Shorter" })).toBeChecked();
      const ways = screen.getByRole("group", { name: "Write it another way" });
      expect(within(ways).queryByRole("button", { name: "Shorter" })).toBeNull();
      expect(within(ways).getByRole("button", { name: "Longer" })).toBeInTheDocument();

      // Back to the first, as it was left; then the shorter one, as edited.
      fireEvent.change(screen.getByDisplayValue("thursday!"), { target: { value: "thursday!!" } });
      fireEvent.click(screen.getByRole("radio", { name: "As I first wrote it" }));
      expect(screen.getByDisplayValue("yes, thursday works")).toBeInTheDocument();
      fireEvent.click(screen.getByRole("radio", { name: "Shorter" }));
      expect(screen.getByDisplayValue("thursday!!")).toBeInTheDocument();
    });

    it("records the way used as sent, which decides the rest", async () => {
      await writeTwoWays();
      fireEvent.click(screen.getByRole("button", { name: "I sent this" }));
      await waitFor(() =>
        expect(native.resolveDraft).toHaveBeenCalledWith("d2", "sent_unedited", "thursday!"),
      );
      expect(native.resolveDraft).toHaveBeenCalledTimes(1);
    });

    it("puts one way aside on its own, and all of them with None of these", async () => {
      await writeTwoWays();
      fireEvent.click(screen.getByRole("button", { name: "Not this one" }));
      await waitFor(() =>
        expect(native.resolveDraft).toHaveBeenCalledWith("d2", "discarded", null),
      );
      expect(await screen.findByDisplayValue("yes, thursday works")).toBeInTheDocument();
      expect(screen.queryByRole("radio")).toBeNull();

      native.writeAnotherDraft.mockResolvedValueOnce({
        ...draft("d3", "Thursday suits me well."),
        alternativeTo: "d1",
        context: { adjustment: "moreProfessional" },
      });
      fireEvent.click(screen.getByRole("button", { name: "More professional" }));
      expect(await screen.findByDisplayValue("Thursday suits me well.")).toBeInTheDocument();
      fireEvent.click(screen.getByRole("button", { name: "None of these" }));
      await waitFor(() =>
        expect(native.resolveDraft).toHaveBeenCalledWith("d1", "discarded", null),
      );
      await waitFor(() =>
        expect(screen.queryByRole("button", { name: "None of these" })).toBeNull(),
      );
    });

    it("uses nothing in the set while another way is being written", async () => {
      native.generateDraft.mockResolvedValueOnce(draft("d1", "yes, thursday works"));
      let finish: (d: Draft) => void = () => undefined;
      native.writeAnotherDraft.mockImplementationOnce(
        () =>
          new Promise<Draft>((done) => {
            finish = done;
          }),
      );
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
      const ways = screen.getByRole("group", { name: "Write it another way" });
      fireEvent.click(within(ways).getByRole("button", { name: "Shorter" }));
      await waitFor(() =>
        expect(screen.getByRole("button", { name: "I sent this" })).toBeDisabled(),
      );
      expect(screen.getByRole("button", { name: "Discard" })).toBeDisabled();

      finish(shorter());
      expect(await screen.findByDisplayValue("thursday!")).toBeInTheDocument();
      expect(screen.getByRole("button", { name: "I sent this" })).toBeEnabled();
      expect(native.resolveDraft).not.toHaveBeenCalled();
    });

    it("writes the whole set again on Regenerate, putting it aside as passed over", async () => {
      await writeTwoWays();
      native.generateDraft.mockResolvedValueOnce(draft("d4", "thursday works for me"));
      fireEvent.click(screen.getByRole("button", { name: "Regenerate" }));
      expect(await screen.findByDisplayValue("thursday works for me")).toBeInTheDocument();
      expect(native.resolveDraft).toHaveBeenCalledWith("d1", "regenerated", null);
      expect(native.resolveDraft).toHaveBeenCalledTimes(1);
      expect(screen.queryByRole("radio")).toBeNull();
    });
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

    it("puts the whole set aside when another way copied away didn't go", async () => {
      await draftFromAnywhere();
      native.writeAnotherDraft.mockResolvedValueOnce({
        ...draft("d2", "yes! 7 it is"),
        alternativeTo: "d1",
        context: { adjustment: "moreCasual" },
      });
      const ways = screen.getByRole("group", { name: "Write it another way" });
      fireEvent.click(within(ways).getByRole("button", { name: "More casual" }));
      expect(await screen.findByDisplayValue("yes! 7 it is")).toBeInTheDocument();
      fireEvent.click(screen.getByRole("button", { name: "Copy and go back" }));
      await waitFor(() => expect(writeText).toHaveBeenCalledWith("yes! 7 it is"));
      fireEvent.click(await screen.findByRole("button", { name: "It didn’t go" }));
      await waitFor(() =>
        expect(native.resolveDraft).toHaveBeenCalledWith("d1", "discarded", null),
      );
      expect(native.resolveDraft).toHaveBeenCalledWith("d2", "discarded", null);
      expect(native.resolveDraft).toHaveBeenCalledTimes(2);
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
