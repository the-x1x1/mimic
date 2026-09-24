import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Filing, SituationSummary } from "@mimic/contracts";
import { MessageFiling } from "../MessageFiling";

// The real hooks run; only the native calls are replaced.
const native = vi.hoisted(() => ({
  situations: vi.fn(),
  decideSituations: vi.fn(),
  letRulesDecide: vi.fn(),
}));
vi.mock("@/lib/ipc", () => ({ ipc: native }));

const situation = (id: string, label: string): SituationSummary => ({
  id,
  label,
  layerLabel: label,
  ownMessages: 0,
  measurable: false,
});

function renderIt(filing: Filing) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <MessageFiling messageId="m1" filing={filing} excerpt="can't make it friday" />
    </QueryClientProvider>,
  );
}

const change = () =>
  screen.getByRole("button", { name: /^(Change|What was this doing\?) “can't make it friday”$/ });

describe("what one of your messages was doing, and saying otherwise", () => {
  beforeEach(() => {
    for (const f of Object.values(native)) f.mockReset();
    native.situations.mockResolvedValue([
      situation("declining", "Saying no"),
      situation("apologising", "Apologising"),
      situation("thanking", "Saying thanks"),
    ]);
  });

  it("says what it is filed under and by whom, and keeps what the user says instead", async () => {
    native.decideSituations.mockResolvedValue({ by: "you", situations: ["apologising"] });
    renderIt({ by: "rules", situations: ["declining"] });
    expect(await screen.findByText(/Saying no, by the rules\./)).toBeInTheDocument();
    fireEvent.click(change());
    expect(
      screen.getByRole("group", { name: /What was this message doing\?/ }),
    ).toBeInTheDocument();
    const no = screen.getByRole("checkbox", { name: "Saying no" });
    expect(no).toBeChecked();
    fireEvent.click(no);
    fireEvent.click(screen.getByRole("checkbox", { name: "Apologising" }));
    // Only a decision someone made can be handed back.
    expect(screen.queryByRole("button", { name: "Let the rules decide" })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() =>
      expect(native.decideSituations).toHaveBeenCalledWith("m1", ["apologising"]),
    );
    expect(await screen.findByText(/Apologising, as you said\./)).toBeInTheDocument();
    expect(screen.queryByRole("group")).toBeNull();
  });

  it("hands a decision back to the rules", async () => {
    native.letRulesDecide.mockResolvedValue({ by: "rules", situations: ["declining"] });
    renderIt({ by: "you", situations: [] });
    expect(await screen.findByText(/Doing none of these, as you said\./)).toBeInTheDocument();
    fireEvent.click(change());
    fireEvent.click(screen.getByRole("button", { name: "Let the rules decide" }));
    await waitFor(() => expect(native.letRulesDecide).toHaveBeenCalledWith("m1"));
    expect(await screen.findByText(/Saying no, by the rules\./)).toBeInTheDocument();
  });

  it("only asks, under a message the rules filed under nothing, and takes none as an answer", async () => {
    native.decideSituations.mockResolvedValue({ by: "you", situations: [] });
    renderIt({ by: "rules", situations: [] });
    expect(change()).toHaveTextContent("What was this doing?");
    expect(screen.queryByText(/none of these/)).toBeNull();
    fireEvent.click(change());
    await screen.findByRole("checkbox", { name: "Saying thanks" });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(native.decideSituations).toHaveBeenCalledWith("m1", []));
    expect(await screen.findByText(/Doing none of these, as you said\./)).toBeInTheDocument();
  });

  it("says what a model on this computer read", async () => {
    renderIt({ by: "model", situations: ["thanking", "apologising"] });
    expect(
      await screen.findByText(
        /Saying thanks · Apologising, as the model on this computer read it\./,
      ),
    ).toBeInTheDocument();
  });
});
