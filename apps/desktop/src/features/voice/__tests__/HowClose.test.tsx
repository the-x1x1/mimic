import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { EvaluationView, Job, ProviderState } from "@mimic/contracts";
import { CASES, HowClose } from "../HowClose";

// The real hooks run; only the native calls are replaced.
const native = vi.hoisted(() => ({
  evaluation: vi.fn(),
  startEvaluation: vi.fn(),
  jobs: vi.fn(),
  providerState: vi.fn(),
}));
vi.mock("@/lib/ipc", () => ({ ipc: native }));

const providers: ProviderState = {
  providers: [
    {
      id: "local",
      displayName: "Local model",
      local: true,
      model: "llama3.2:3b",
      description: "",
      requiresCredential: false,
    },
  ],
  active: "local",
  configuredSecrets: [],
  credentials: { protection: "account", unsealedLeft: false, problem: null },
} as unknown as ProviderState;

const view: EvaluationView = {
  id: "e1",
  createdAt: "2026-09-22T08:00:00.000Z",
  provider: "local",
  model: "llama3.2:3b",
  measured: 6,
  remaining: 6,
  strategy: "conversation_grouped",
  conversations: 6,
  heldOutConversations: 2,
  measuredConversations: 2,
  warnings: [
    "only two conversations: one trains, one is held out, so the score rests on a single thread",
  ],
  embeddingProvider: "lexical_v1",
  commonReply: { text: "Sounds good!", times: 14 },
  systems: [
    {
      system: "mimic",
      cases: 6,
      length: { mean: 0.72, p10: 0.4 },
      vocabulary: { mean: 0.31, p10: 0.1 },
      punctuation: { mean: 0.9, p10: 0.6 },
      embedding: { mean: 0.55, p10: 0.3 },
    },
    {
      system: "generic",
      cases: 6,
      length: { mean: 0.35, p10: 0.1 },
      vocabulary: { mean: 0.2, p10: 0.05 },
      punctuation: { mean: 0.6, p10: 0.4 },
      embedding: { mean: 0.4, p10: 0.2 },
    },
    {
      system: "common_reply",
      cases: 6,
      length: { mean: 0.2, p10: 0.05 },
      vocabulary: { mean: 0.05, p10: 0 },
      punctuation: { mean: 0.7, p10: 0.4 },
      embedding: { mean: 0.15, p10: 0.05 },
    },
  ],
  cases: [
    {
      incoming: "can you send the deck?",
      from: "Ada Lovelace",
      reply: "sending it tonight",
      answers: [
        { system: "mimic", text: "will send it tonight" },
        { system: "generic", text: "Certainly! I will send the deck shortly." },
        { system: "common_reply", text: "Sounds good!" },
      ],
    },
  ],
};

const job = (over: Partial<Job>): Job => ({
  id: "j1",
  type: "evaluate_drafts",
  status: "running",
  payload: {},
  progressCurrent: 3,
  progressTotal: 12,
  phase: "writing replies",
  resumable: false,
  createdAt: "2026-09-22T09:00:00.000Z",
  startedAt: null,
  heartbeatAt: null,
  completedAt: null,
  result: null,
  error: null,
  ...over,
});

function renderIt() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <HowClose />
    </QueryClientProvider>,
  );
}

describe("how close my drafts come", () => {
  beforeEach(() => {
    for (const f of Object.values(native)) f.mockReset();
    native.providerState.mockResolvedValue(providers);
    native.jobs.mockResolvedValue([]);
  });

  it("offers to measure, saying what it will ask the model for, before the first run", async () => {
    native.evaluation.mockResolvedValue(null);
    native.startEvaluation.mockResolvedValue(job({ status: "queued" }));
    renderIt();
    expect(await screen.findByText(/I hold back some of your conversations/)).toBeInTheDocument();
    expect(
      await screen.findByText(
        `It asks Local model for two replies to each of up to ${CASES} messages people sent you. Nothing leaves this computer. Mail isn't checked until it's done.`,
      ),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Measure it" }));
    await waitFor(() => expect(native.startEvaluation).toHaveBeenCalledTimes(1));
  });

  it("shows every measure for every way of answering, and adds none of them up", async () => {
    native.evaluation.mockResolvedValue(view);
    renderIt();
    const table = await screen.findByRole("table");
    const headers = within(table)
      .getAllByRole("columnheader")
      .map((h) => h.textContent);
    expect(headers).toEqual(["Measure", "My drafts", "A generic reply", "Your most common reply"]);
    const length = within(table).getByRole("row", { name: /^Length/ });
    expect(
      within(length)
        .getAllByRole("cell")
        .map((c) => c.textContent),
    ).toEqual(["72%10th percentile 40%", "35%10th percentile 10%", "20%10th percentile 5%"]);
    expect(within(table).getAllByRole("row")).toHaveLength(5);
    expect(screen.queryByText(/score/i)).toBeNull();
    expect(
      screen.getByText(/compares the words and letters used, not what they mean/),
    ).toBeInTheDocument();
    expect(screen.getByText(/that leans against me/)).toBeInTheDocument();
    expect(
      screen.getByText(/Your most common reply is “Sounds good!”, sent 14 times/),
    ).toBeInTheDocument();
    expect(
      screen.getByText(/Only two of your conversations had replies to measure/),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Measure again" })).toBeEnabled();
  });

  it("shows the replies side by side on request", async () => {
    native.evaluation.mockResolvedValue(view);
    renderIt();
    fireEvent.click(await screen.findByRole("button", { name: "Show the replies" }));
    expect(screen.getByText("Ada wrote")).toBeInTheDocument();
    expect(screen.getByText("sending it tonight")).toBeInTheDocument();
    expect(screen.getByText("I wrote")).toBeInTheDocument();
    expect(screen.getByText("will send it tonight")).toBeInTheDocument();
    expect(screen.getByText("Certainly! I will send the deck shortly.")).toBeInTheDocument();
  });

  it("shows a run in progress, and why the last one failed", async () => {
    native.evaluation.mockResolvedValue(view);
    native.jobs.mockResolvedValueOnce([job({})]);
    const { unmount } = renderIt();
    expect(await screen.findByRole("progressbar", { name: "Writing replies" })).toBeInTheDocument();
    expect(await screen.findByRole("button", { name: "Measure again" })).toBeDisabled();
    unmount();

    native.jobs.mockResolvedValue([
      job({
        status: "failed",
        error: {
          code: "failed",
          message: "The part of Mimic that does the measuring isn't running",
        },
      }),
    ]);
    renderIt();
    expect(
      await screen.findByText("The part of Mimic that does the measuring isn't running"),
    ).toBeInTheDocument();
  });

  it("says when the last run was stopped before it finished", async () => {
    native.evaluation.mockResolvedValue(view);
    native.jobs.mockResolvedValue([job({ status: "canceled" })]);
    renderIt();
    expect(
      await screen.findByText(/The last measurement was stopped before it finished/),
    ).toBeInTheDocument();
  });

  it("says so when a hosted model will be sent people's messages", async () => {
    native.evaluation.mockResolvedValue(null);
    native.providerState.mockResolvedValue({
      ...providers,
      providers: [
        { ...providers.providers[0]!, id: "anthropic", displayName: "Claude", local: false },
      ],
      active: "anthropic",
    });
    renderIt();
    expect(
      await screen.findByText(
        /the messages before each in its conversation, and some of your past replies with the messages they answered are sent to Claude, and billed/,
      ),
    ).toBeInTheDocument();
  });

  it("says when replies it was measured on no longer count", async () => {
    native.evaluation.mockResolvedValue({ ...view, remaining: 0, cases: [] });
    renderIt();
    expect(
      await screen.findByText(/None of the replies this was measured on count any more/),
    ).toBeInTheDocument();
    expect(screen.queryByRole("table")).toBeNull();
  });
});
