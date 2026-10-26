import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { AutoReplyRules } from "@mimic/contracts";
import { ByMyself } from "../ByMyself";

// The real hooks run; only the native calls are replaced.
const native = vi.hoisted(() => ({ autoReply: vi.fn(), setAutoReply: vi.fn() }));
vi.mock("@/lib/ipc", () => ({ ipc: native }));

const rules: AutoReplyRules = {
  enabled: true,
  mailboxes: ["s1"],
  who: "known",
  people: [],
  never: [],
  holdMinutes: 10,
  dailyLimit: 10,
  enabledAt: "2026-10-26T09:00:00.000Z",
};

function renderIt(r: AutoReplyRules, participantId: string | null = "p-ada") {
  native.autoReply.mockResolvedValue({ rules: r, running: true, lastDay: 0 });
  native.setAutoReply.mockImplementation(async (next: AutoReplyRules) => ({
    rules: next,
    running: true,
    lastDay: 0,
  }));
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <ByMyself participantId={participantId} />
    </QueryClientProvider>,
  );
}

describe("replying by myself to one person", () => {
  beforeEach(() => {
    for (const f of Object.values(native)) f.mockReset();
  });

  it("says nothing while replying by myself is off", async () => {
    const { container } = renderIt({ ...rules, enabled: false });
    await waitFor(() => expect(native.autoReply).toHaveBeenCalled());
    expect(container).toBeEmptyDOMElement();
  });

  it("puts someone on the never list in one click, and takes them off again", async () => {
    renderIt({ ...rules, people: ["p-ada"] });
    fireEvent.click(await screen.findByRole("button", { name: "Never by myself to them" }));
    await waitFor(() =>
      expect(native.setAutoReply).toHaveBeenLastCalledWith({
        ...rules,
        people: [],
        never: ["p-ada"],
      }),
    );
    fireEvent.click(await screen.findByRole("button", { name: "Let me answer them by myself" }));
    await waitFor(() =>
      expect(native.setAutoReply).toHaveBeenLastCalledWith({ ...rules, people: [], never: [] }),
    );
  });

  it("adds someone to the people chosen when only they are answered", async () => {
    renderIt({ ...rules, who: "listed", people: ["p-bob"] });
    expect(await screen.findByText(/they aren't one/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Answer them by myself too" }));
    await waitFor(() =>
      expect(native.setAutoReply).toHaveBeenLastCalledWith(
        expect.objectContaining({ people: ["p-bob", "p-ada"] }),
      ),
    );
  });
});
