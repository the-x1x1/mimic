import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { AnywhereSection } from "../AnywhereSection";

// The real hooks run; only the native calls are replaced.
const native = vi.hoisted(() => ({
  settings: vi.fn(),
  setSetting: vi.fn(),
}));
vi.mock("@/lib/ipc", () => ({ ipc: native }));

function renderIt() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <AnywhereSection />
    </QueryClientProvider>,
  );
}

describe("answering a message from anywhere", () => {
  beforeEach(() => {
    for (const f of Object.values(native)) f.mockReset();
    native.settings.mockResolvedValue({ "anywhere.shortcut": "" });
  });

  it("is off until a shortcut is chosen, and keeps the one chosen", async () => {
    native.setSetting.mockResolvedValue({});
    renderIt();
    const select = await screen.findByLabelText("Shortcut");
    await waitFor(() => expect(select).toHaveValue(""));
    fireEvent.change(select, { target: { value: "Ctrl+Shift+Alt+M" } });
    await waitFor(() =>
      expect(native.setSetting).toHaveBeenCalledWith("anywhere.shortcut", "Ctrl+Shift+Alt+M"),
    );
  });

  it("says so when another app already has the shortcut", async () => {
    native.setSetting.mockRejectedValue(
      new Error("Another app is already using Ctrl+Shift+Alt+R. Pick another, or turn it off."),
    );
    renderIt();
    const select = await screen.findByLabelText("Shortcut");
    fireEvent.change(select, { target: { value: "Ctrl+Shift+Alt+R" } });
    expect(
      await screen.findByText(
        "Another app is already using Ctrl+Shift+Alt+R. Pick another, or turn it off.",
      ),
    ).toBeInTheDocument();
  });
});
