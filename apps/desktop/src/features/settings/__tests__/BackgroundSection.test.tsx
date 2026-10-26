import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { BackgroundSection } from "../BackgroundSection";

// The real hooks run; only the native calls are replaced.
const native = vi.hoisted(() => ({
  settings: vi.fn(),
  setSetting: vi.fn(),
  startWithWindows: vi.fn(),
  setStartWithWindows: vi.fn(),
}));
vi.mock("@/lib/ipc", () => ({ ipc: native }));

function renderIt() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <BackgroundSection />
    </QueryClientProvider>,
  );
}

describe("running in the background", () => {
  beforeEach(() => {
    for (const f of Object.values(native)) f.mockReset();
    native.settings.mockResolvedValue({ "app.keepRunning": true, "notify.enabled": true });
    native.startWithWindows.mockResolvedValue(false);
    native.setSetting.mockResolvedValue({});
  });

  it("keeps running and tells by default, and starting with Windows is the user's to turn on", async () => {
    renderIt();
    const keep = await screen.findByLabelText(/Keep running in the notification area/);
    await waitFor(() => expect(keep).toBeChecked());
    expect(screen.getByLabelText(/Tell me with a Windows notification/)).toBeChecked();
    const atSignIn = screen.getByLabelText(/Start when I sign in to Windows/);
    await waitFor(() => expect(atSignIn).toBeEnabled());
    expect(atSignIn).not.toBeChecked();

    native.setStartWithWindows.mockResolvedValue(true);
    fireEvent.click(atSignIn);
    await waitFor(() => expect(native.setStartWithWindows).toHaveBeenCalledWith(true));
    await waitFor(() => expect(atSignIn).toBeChecked());

    fireEvent.click(keep);
    expect(native.setSetting).toHaveBeenCalledWith("app.keepRunning", false);
  });

  it("says why when Windows won't change it", async () => {
    native.setStartWithWindows.mockRejectedValue(
      new Error("I couldn't change whether I start with Windows: access denied"),
    );
    renderIt();
    const atSignIn = await screen.findByLabelText(/Start when I sign in to Windows/);
    await waitFor(() => expect(atSignIn).toBeEnabled());
    fireEvent.click(atSignIn);
    expect(
      await screen.findByText("I couldn't change whether I start with Windows: access denied"),
    ).toBeInTheDocument();
  });
});
