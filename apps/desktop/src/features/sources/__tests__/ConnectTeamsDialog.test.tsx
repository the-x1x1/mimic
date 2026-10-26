import type { ReactNode } from "react";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ConnectTeamsDialog, TeamsSignInAgain } from "../ConnectTeamsDialog";

// The real hooks run; only the native calls are replaced.
const native = vi.hoisted(() => ({
  connectTeams: vi.fn(),
  signInTeamsAgain: vi.fn(),
  cancelMailSignIn: vi.fn(),
  mailSignInAvailable: vi.fn(),
}));
vi.mock("@/lib/ipc", () => ({ ipc: native }));

function wrap(node: ReactNode) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(<QueryClientProvider client={client}>{node}</QueryClientProvider>);
}

describe("connecting Teams", () => {
  beforeEach(() => {
    for (const f of Object.values(native)) f.mockReset();
    native.cancelMailSignIn.mockResolvedValue(undefined);
  });

  it("signs in with Microsoft and connects, saying a work account is needed", async () => {
    const onClose = vi.fn();
    native.mailSignInAvailable.mockResolvedValue({ microsoft: true, google: false });
    native.connectTeams.mockResolvedValue({ name: "c@formicaria.example (Teams)" });
    wrap(<ConnectTeamsDialog onClose={onClose} />);
    expect(
      screen.getByText(/a personal\s+Microsoft account.s Teams can.t be read/),
    ).toBeInTheDocument();
    const go = screen.getByRole("button", { name: "Sign in with Microsoft" });
    await waitFor(() => expect(go).toBeEnabled());
    fireEvent.click(go);
    await waitFor(() => expect(native.connectTeams).toHaveBeenCalled());
    await waitFor(() => expect(onClose).toHaveBeenCalled());
  });

  it("offers nothing when this copy can't sign in with Microsoft", async () => {
    native.mailSignInAvailable.mockResolvedValue({ microsoft: false, google: false });
    wrap(<ConnectTeamsDialog onClose={vi.fn()} />);
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Sign in with Microsoft" })).toBeDisabled(),
    );
    expect(native.connectTeams).not.toHaveBeenCalled();
  });

  it("can be stopped while the browser is open, and says nothing of it", async () => {
    native.mailSignInAvailable.mockResolvedValue({ microsoft: true, google: false });
    let refuse: (e: unknown) => void = () => undefined;
    native.connectTeams.mockImplementation(
      () =>
        new Promise((_, reject) => {
          refuse = reject;
        }),
    );
    wrap(<ConnectTeamsDialog onClose={vi.fn()} />);
    const go = screen.getByRole("button", { name: "Sign in with Microsoft" });
    await waitFor(() => expect(go).toBeEnabled());
    fireEvent.click(go);
    fireEvent.click(await screen.findByRole("button", { name: "Stop signing in" }));
    expect(native.cancelMailSignIn).toHaveBeenCalled();
    refuse(Object.assign(new Error("Signing in was stopped."), { code: "canceled" }));
    expect(
      await screen.findByRole("button", { name: "Sign in with Microsoft" }),
    ).toBeInTheDocument();
    expect(screen.queryByText("Signing in was stopped.")).toBeNull();
  });

  it("signs a connected account in again", async () => {
    native.signInTeamsAgain.mockResolvedValue({});
    wrap(<TeamsSignInAgain sourceId="s7" name="c@formicaria.example (Teams)" />);
    fireEvent.click(screen.getByRole("button", { name: "Sign in again" }));
    await waitFor(() => expect(native.signInTeamsAgain).toHaveBeenCalledWith("s7"));
  });
});
