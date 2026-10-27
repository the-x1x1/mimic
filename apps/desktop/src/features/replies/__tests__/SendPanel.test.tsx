import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Draft, OutboxEntry, ReplyEnvelope } from "@mimic/contracts";
import { DraftReview } from "../RepliesPage";

// The real hooks run; only the native calls are replaced.
const native = vi.hoisted(() => ({
  replyEnvelope: vi.fn(),
  sendReply: vi.fn(),
  resolveDraft: vi.fn(),
  writeAnotherDraft: vi.fn(),
  addDraftPreference: vi.fn(),
  settings: vi.fn(),
}));
vi.mock("@/lib/ipc", () => ({ ipc: native }));

const draft: Draft = {
  id: "d1",
  participantId: "p1",
  conversationId: "c1",
  channel: "email",
  situationId: null,
  incomingMessage: "can you confirm Friday?",
  intent: "yes",
  generatedText: "yes, friday works",
  finalText: null,
  provider: "local",
  model: "llama3.1:8b",
  context: {},
  promptHash: "abc",
  evidence: {},
  createdAt: "2026-10-20T10:00:00Z",
  resolvedAt: null,
  outcome: null,
  incomingMessageId: "m2",
  alternativeTo: null,
};

const envelope = (over: Partial<ReplyEnvelope> = {}): ReplyEnvelope => ({
  draftId: "d1",
  mailboxes: [
    {
      sourceId: "s1",
      name: "c@example.com",
      fromAddress: "c@example.com",
      server: "smtp.gmail.com",
      whyNot: null,
    },
  ],
  mailbox: "s1",
  to: ["ada@work.example"],
  otherAddresses: ["ada@example.com"],
  othersOnIt: [],
  subject: "Re: Friday",
  whyNot: null,
  held: null,
  ...over,
});

const sent = (to: string[], cc: string[] = []): OutboxEntry => ({
  id: "o1",
  draftId: "d1",
  sourceId: "s1",
  conversationId: "c1",
  answersMessageId: "m2",
  participantId: "p1",
  fromAddress: "c@example.com",
  to,
  cc,
  subject: "Re: Friday",
  body: "yes, friday works",
  messageId: "x@example.com",
  how: "approved",
  status: "sent",
  sendAfter: "2026-10-20T10:01:00Z",
  error: null,
  createdAt: "2026-10-20T10:01:00Z",
  updatedAt: "2026-10-20T10:01:01Z",
  sentAt: "2026-10-20T10:01:01Z",
});

/** Press Send it once it can be: after the wait chosen has been read. */
async function sendIt(panel: HTMLElement) {
  const go = within(panel).getByRole("button", { name: "Send it" });
  await waitFor(() => expect(go).toBeEnabled());
  fireEvent.click(go);
}

function renderReview(onDone = vi.fn()) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <DraftReview draft={draft} onDone={onDone} />
    </QueryClientProvider>,
  );
  return onDone;
}

describe("sending a reply from Mimic", () => {
  beforeEach(() => {
    for (const f of Object.values(native)) f.mockReset();
    // At once, unless a test says otherwise.
    native.settings.mockResolvedValue({ "send.undoSeconds": 0 });
  });

  it("shows who it goes to and through what before anything goes, and sends exactly that", async () => {
    native.replyEnvelope.mockResolvedValue(envelope());
    native.sendReply.mockImplementation(async (r: { to: string[] }) => sent(r.to));
    const onDone = renderReview();
    fireEvent.click(await screen.findByRole("button", { name: "Send" }));
    expect(native.sendReply).not.toHaveBeenCalled();

    const panel = screen.getByRole("group", { name: "Send it" });
    expect(within(panel).getByText("c@example.com")).toBeInTheDocument();
    expect(within(panel).getByText(/through smtp.gmail.com/)).toBeInTheDocument();
    expect(within(panel).getByText(/not to anyone else on the thread/)).toBeInTheDocument();
    const to = within(panel).getByLabelText("To");
    expect(
      within(to)
        .getAllByRole("option")
        .map((o) => o.textContent),
    ).toEqual(["ada@work.example", "ada@example.com"]);
    fireEvent.change(to, { target: { value: "1" } });
    fireEvent.change(within(panel).getByLabelText("Subject"), {
      target: { value: "Re: Friday (and Saturday)" },
    });
    await sendIt(panel);

    await waitFor(() => expect(onDone).toHaveBeenCalled());
    expect(native.sendReply).toHaveBeenCalledWith({
      draftId: "d1",
      text: "yes, friday works",
      sourceId: "s1",
      to: ["ada@example.com"],
      cc: [],
      subject: "Re: Friday (and Saturday)",
    });
    expect(native.resolveDraft).not.toHaveBeenCalled();
  });

  it("sends a reply to a Slack message into that conversation, as the user, with no subject", async () => {
    native.replyEnvelope.mockResolvedValue(
      envelope({
        kind: "slack",
        mailboxes: [
          {
            sourceId: "s9",
            name: "Acme (Slack)",
            fromAddress: "c in Acme",
            server: "Slack",
            whyNot: null,
          },
        ],
        mailbox: "s9",
        to: ["Ada Lovelace"],
        otherAddresses: [],
        subject: "",
      }),
    );
    native.sendReply.mockImplementation(async (r: { to: string[] }) => ({
      ...sent(r.to),
      sourceId: "s9",
      fromAddress: "slack:T1:U1",
      subject: "",
    }));
    const onDone = renderReview();
    fireEvent.click(await screen.findByRole("button", { name: "Send" }));
    const panel = screen.getByRole("group", { name: "Send it" });
    expect(within(panel).getByText("c in Acme")).toBeInTheDocument();
    expect(within(panel).getByText("Ada Lovelace")).toBeInTheDocument();
    expect(within(panel).queryByLabelText("Subject")).toBeNull();
    expect(within(panel).getByText(/into that conversation in Slack, as you/)).toBeInTheDocument();
    await sendIt(panel);
    await waitFor(() => expect(onDone).toHaveBeenCalled());
    expect(native.sendReply).toHaveBeenCalledWith({
      draftId: "d1",
      text: "yes, friday works",
      sourceId: "s9",
      to: ["Ada Lovelace"],
      cc: [],
      subject: "",
    });
  });

  it("sends a reply to a Teams message into that chat", async () => {
    native.replyEnvelope.mockResolvedValue(
      envelope({
        kind: "teams",
        mailboxes: [
          {
            sourceId: "s8",
            name: "c@formicaria.example (Teams)",
            fromAddress: "C Haynes in Teams",
            server: "Teams",
            whyNot: null,
          },
        ],
        mailbox: "s8",
        to: ["Ada Lovelace"],
        otherAddresses: [],
        subject: "",
      }),
    );
    native.sendReply.mockImplementation(async (r: { to: string[] }) => ({
      ...sent(r.to),
      sourceId: "s8",
      fromAddress: "teams:u1",
      subject: "",
    }));
    const onDone = renderReview();
    fireEvent.click(await screen.findByRole("button", { name: "Send" }));
    const panel = screen.getByRole("group", { name: "Send it" });
    expect(within(panel).getByText("C Haynes in Teams")).toBeInTheDocument();
    expect(within(panel).queryByLabelText("Subject")).toBeNull();
    expect(within(panel).getByText(/into that conversation in Teams, as you/)).toBeInTheDocument();
    await sendIt(panel);
    await waitFor(() => expect(onDone).toHaveBeenCalled());
    expect(native.sendReply).toHaveBeenCalledWith(expect.objectContaining({ sourceId: "s8" }));
  });

  it("sends a reply to a Telegram message into that chat", async () => {
    native.replyEnvelope.mockResolvedValue(
      envelope({
        kind: "telegram",
        mailboxes: [
          {
            sourceId: "s10",
            name: "C Haynes (Telegram)",
            fromAddress: "C Haynes on Telegram",
            server: "Telegram",
            whyNot: null,
          },
        ],
        mailbox: "s10",
        to: ["Ada Lovelace"],
        otherAddresses: [],
        subject: "",
      }),
    );
    native.sendReply.mockImplementation(async (r: { to: string[] }) => ({
      ...sent(r.to),
      sourceId: "s10",
      fromAddress: "telegram:user7",
      subject: "",
    }));
    const onDone = renderReview();
    fireEvent.click(await screen.findByRole("button", { name: "Send" }));
    const panel = screen.getByRole("group", { name: "Send it" });
    expect(within(panel).getByText("C Haynes on Telegram")).toBeInTheDocument();
    expect(within(panel).queryByLabelText("Subject")).toBeNull();
    expect(
      within(panel).getByText(/into that conversation in Telegram, as you/),
    ).toBeInTheDocument();
    await sendIt(panel);
    await waitFor(() => expect(onDone).toHaveBeenCalled());
    expect(native.sendReply).toHaveBeenCalledWith(expect.objectContaining({ sourceId: "s10" }));
  });

  it("copies in only the others on their message that are ticked", async () => {
    native.replyEnvelope.mockResolvedValue(
      envelope({ othersOnIt: ["bob@example.com", "cy@example.com", "di@example.com"] }),
    );
    native.sendReply.mockImplementation(async (r: { to: string[]; cc: string[] }) =>
      sent(r.to, r.cc),
    );
    renderReview();
    fireEvent.click(await screen.findByRole("button", { name: "Send" }));
    const panel = screen.getByRole("group", { name: "Send it" });
    const copy = within(panel).getByRole("group", { name: "Copy in" });
    const boxes = within(copy).getAllByRole("checkbox");
    expect(boxes.map((b) => (b as HTMLInputElement).checked)).toEqual([false, false, false, false]);
    expect(
      within(panel).getByText(/not to anyone else on the thread unless you copy them in/),
    ).toBeInTheDocument();

    fireEvent.click(within(copy).getByRole("checkbox", { name: "Everyone else on their message" }));
    expect(
      within(panel).getByText(/copying in bob@example.com, cy@example.com and di@example.com/),
    ).toBeInTheDocument();
    fireEvent.click(within(copy).getByRole("checkbox", { name: "cy@example.com" }));
    expect(
      within(copy).getByRole("checkbox", { name: "Everyone else on their message" }),
    ).not.toBeChecked();
    await sendIt(panel);

    await waitFor(() => expect(native.sendReply).toHaveBeenCalled());
    expect(native.sendReply.mock.calls[0]?.[0]).toMatchObject({
      to: ["ada@work.example"],
      cc: ["bob@example.com", "di@example.com"],
    });
  });

  it("shows the signature the reply goes with", async () => {
    native.replyEnvelope.mockResolvedValue(
      envelope({
        mailboxes: [
          {
            sourceId: "s1",
            name: "c@example.com",
            fromAddress: "c@example.com",
            server: "smtp.gmail.com",
            whyNot: null,
            signature: "C Haynes\nFormicaria",
          },
        ],
      }),
    );
    renderReview();
    fireEvent.click(await screen.findByRole("button", { name: "Send" }));
    const panel = screen.getByRole("group", { name: "Send it" });
    expect(within(panel).getByText("Signed")).toBeInTheDocument();
    expect(within(panel).getByText(/C Haynes\s+Formicaria/)).toBeInTheDocument();
  });

  it("sends the words as changed on screen", async () => {
    native.replyEnvelope.mockResolvedValue(envelope({ otherAddresses: [] }));
    native.sendReply.mockImplementation(async (r: { to: string[] }) => sent(r.to));
    renderReview();
    await screen.findByRole("button", { name: "Send" });
    fireEvent.click(screen.getByRole("button", { name: "Change it" }));
    fireEvent.change(screen.getByRole("textbox", { name: /Change whatever you like/ }), {
      target: { value: "yes — friday at nine" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Send" }));
    const panel = screen.getByRole("group", { name: "Send it" });
    expect(within(panel).getByText("ada@work.example")).toBeInTheDocument();
    await sendIt(panel);
    await waitFor(() => expect(native.sendReply).toHaveBeenCalled());
    expect(native.sendReply.mock.calls[0]?.[0]).toMatchObject({ text: "yes — friday at nine" });
  });

  it("offers no Send where it can't go, and says why", async () => {
    native.replyEnvelope.mockResolvedValue(
      envelope({ mailbox: null, whyNot: "Set up sending for it under Your mail." }),
    );
    renderReview();
    expect(await screen.findByText(/Set up sending for it under Your mail/)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Send" })).toBeNull();
    expect(screen.getByRole("button", { name: "Use this" })).toBeInTheDocument();
  });

  it("won't offer to send again a reply that may already have gone", async () => {
    native.replyEnvelope.mockResolvedValue(envelope());
    native.sendReply.mockRejectedValue(
      Object.assign(new Error("It may or may not have gone. Look in your Sent folder."), {
        code: "send_unsure",
      }),
    );
    const onDone = renderReview();
    fireEvent.click(await screen.findByRole("button", { name: "Send" }));
    const panel = screen.getByRole("group", { name: "Send it" });
    await sendIt(panel);
    expect(await within(panel).findByText(/I won't send it again by myself/)).toBeInTheDocument();
    expect(within(panel).getByRole("button", { name: "Send it" })).toBeDisabled();
    expect(onDone).not.toHaveBeenCalled();
  });

  it("waits the seconds chosen before it goes, and can be taken back", async () => {
    native.settings.mockResolvedValue({ "send.undoSeconds": 1 });
    native.replyEnvelope.mockResolvedValue(envelope());
    native.sendReply.mockImplementation(async (r: { to: string[] }) => sent(r.to));
    renderReview();
    fireEvent.click(await screen.findByRole("button", { name: "Send" }));
    const panel = screen.getByRole("group", { name: "Send it" });
    await sendIt(panel);
    expect(within(panel).getByRole("status")).toHaveTextContent("Sending in 1 second");
    expect(within(panel).getByLabelText("Subject")).toBeDisabled();
    fireEvent.click(within(panel).getByRole("button", { name: "Don’t send it" }));
    await new Promise((r) => setTimeout(r, 1500));
    expect(native.sendReply).not.toHaveBeenCalled();
    expect(within(panel).getByRole("button", { name: "Send it" })).toBeEnabled();

    await sendIt(panel);
    await waitFor(() => expect(native.sendReply).toHaveBeenCalledTimes(1), { timeout: 3000 });
  }, 10_000);
});
