import type { ReactNode } from "react";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  ConnectGoogleChatDialog,
  GoogleChatSignInAgain,
  GoogleChatSpaces,
} from "../ConnectGoogleChatDialog";

// The real hooks run; only the native calls are replaced.
const native = vi.hoisted(() => ({
  connectGoogleChat: vi.fn(),
  setGoogleChatSpaces: vi.fn(),
  signInGoogleChatAgain: vi.fn(),
  cancelMailSignIn: vi.fn(),
  mailSignInAvailable: vi.fn(),
}));
vi.mock("@/lib/ipc", () => ({ ipc: native }));

function wrap(node: ReactNode) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(<QueryClientProvider client={client}>{node}</QueryClientProvider>);
}

describe("connecting Google Chat", () => {
  beforeEach(() => {
    for (const f of Object.values(native)) f.mockReset();
    native.cancelMailSignIn.mockResolvedValue(undefined);
  });

  it("signs in with Google and connects, saying a Workspace account is needed", async () => {
    const onClose = vi.fn();
    native.mailSignInAvailable.mockResolvedValue({ microsoft: false, google: true });
    native.connectGoogleChat.mockResolvedValue({ name: "c@formicaria.example (Google Chat)" });
    wrap(<ConnectGoogleChatDialog onClose={onClose} />);
    expect(screen.getByText(/a personal Gmail account has no Google Chat API/)).toBeInTheDocument();
    expect(screen.getByText(/turn on the Google Chat API/)).toBeInTheDocument();
    const go = screen.getByRole("button", { name: "Sign in with Google" });
    await waitFor(() => expect(go).toBeEnabled());
    fireEvent.click(go);
    // Named spaces aren't read unless asked.
    await waitFor(() => expect(native.connectGoogleChat).toHaveBeenCalledWith(false));
    await waitFor(() => expect(onClose).toHaveBeenCalled());
  });

  it("reads named spaces only when the box is ticked, and says which threads", async () => {
    native.mailSignInAvailable.mockResolvedValue({ microsoft: false, google: true });
    native.connectGoogleChat.mockResolvedValue({ name: "c@formicaria.example (Google Chat)" });
    wrap(<ConnectGoogleChatDialog onClose={vi.fn()} />);
    expect(screen.getByText(/named spaces only if you tick the box/)).toBeInTheDocument();
    expect(screen.getByText(/only the threads you're in/)).toBeInTheDocument();
    const box = screen.getByRole("checkbox", { name: /Also read named spaces/ });
    expect(box).not.toBeChecked();
    fireEvent.click(box);
    const go = screen.getByRole("button", { name: "Sign in with Google" });
    await waitFor(() => expect(go).toBeEnabled());
    fireEvent.click(go);
    await waitFor(() => expect(native.connectGoogleChat).toHaveBeenCalledWith(true));
  });

  it("turns reading a connected account's spaces on and off", async () => {
    native.setGoogleChatSpaces.mockResolvedValue({});
    wrap(<GoogleChatSpaces sourceId="s9" on={false} />);
    expect(screen.getByText(/What I read from spaces before stays/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("checkbox", { name: /Read named spaces/ }));
    await waitFor(() => expect(native.setGoogleChatSpaces).toHaveBeenCalledWith("s9", true));
  });

  it("offers nothing when this copy can't sign in with Google", async () => {
    native.mailSignInAvailable.mockResolvedValue({ microsoft: true, google: false });
    wrap(<ConnectGoogleChatDialog onClose={vi.fn()} />);
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Sign in with Google" })).toBeDisabled(),
    );
    expect(await screen.findByText(/isn't set up to sign in with Google yet/)).toBeInTheDocument();
    expect(native.connectGoogleChat).not.toHaveBeenCalled();
  });

  it("can be stopped while the browser is open, and says nothing of it", async () => {
    native.mailSignInAvailable.mockResolvedValue({ microsoft: false, google: true });
    let refuse: (e: unknown) => void = () => undefined;
    native.connectGoogleChat.mockImplementation(
      () =>
        new Promise((_, reject) => {
          refuse = reject;
        }),
    );
    wrap(<ConnectGoogleChatDialog onClose={vi.fn()} />);
    const go = screen.getByRole("button", { name: "Sign in with Google" });
    await waitFor(() => expect(go).toBeEnabled());
    fireEvent.click(go);
    fireEvent.click(await screen.findByRole("button", { name: "Stop signing in" }));
    expect(native.cancelMailSignIn).toHaveBeenCalled();
    refuse(Object.assign(new Error("Signing in was stopped."), { code: "canceled" }));
    expect(await screen.findByRole("button", { name: "Sign in with Google" })).toBeInTheDocument();
    expect(screen.queryByText("Signing in was stopped.")).toBeNull();
  });

  it("signs a connected account in again", async () => {
    native.signInGoogleChatAgain.mockResolvedValue({});
    wrap(<GoogleChatSignInAgain sourceId="s9" name="c@formicaria.example (Google Chat)" />);
    fireEvent.click(screen.getByRole("button", { name: "Sign in again" }));
    await waitFor(() => expect(native.signInGoogleChatAgain).toHaveBeenCalledWith("s9"));
  });
});
