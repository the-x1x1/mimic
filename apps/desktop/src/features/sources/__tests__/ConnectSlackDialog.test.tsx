import type { ReactNode } from "react";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ConnectSlackDialog, SlackChannels, SlackToken } from "../ConnectSlackDialog";

// The real hooks run; only the native calls are replaced.
const native = vi.hoisted(() => ({
  connectSlack: vi.fn(),
  setSlackToken: vi.fn(),
  setSlackChannels: vi.fn(),
}));
vi.mock("@/lib/ipc", () => ({ ipc: native }));

const TOKEN = "xoxp-1111111111-2222222222-abcdefabcdef";

function wrap(node: ReactNode) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(<QueryClientProvider client={client}>{node}</QueryClientProvider>);
}

describe("connecting Slack", () => {
  beforeEach(() => {
    native.connectSlack.mockReset();
    native.setSlackToken.mockReset();
    native.setSlackChannels.mockReset();
  });

  it("reads the channels you're in only when ticked, and says what a token without the scopes needs", async () => {
    native.setSlackChannels.mockResolvedValue({});
    wrap(<SlackChannels sourceId="src-9" on={false} missing={null} />);
    const box = screen.getByRole("checkbox", { name: /Read the channels I.m in/ });
    expect(box).not.toBeChecked();
    expect(
      screen.getByText(/Off: I read direct messages and group messages only/),
    ).toBeInTheDocument();
    fireEvent.click(box);
    await waitFor(() => expect(native.setSlackChannels).toHaveBeenCalledWith("src-9", true));
  });

  it("says which scopes to add when the token can't read channels", () => {
    wrap(<SlackChannels sourceId="src-9" on missing="channels:read" />);
    expect(screen.getByText(/only the threads you.re in/)).toBeInTheDocument();
    const alert = screen.getByRole("alert");
    expect(alert.textContent).toContain("(channels:read)");
    expect(alert.textContent).toContain(
      "channels:history, channels:read, groups:history, groups:read",
    );
  });

  it("shows the manifest, and connects only with a user token", async () => {
    const onClose = vi.fn();
    native.connectSlack.mockResolvedValue({ name: "Acme (Slack)" });
    wrap(<ConnectSlackDialog onClose={onClose} />);
    expect(screen.getByLabelText("Slack app manifest").textContent).toContain("im:history");
    expect(screen.getByLabelText("Slack app manifest").textContent).toContain("channels:history");
    const connect = screen.getByRole("button", { name: "Connect and start reading" });
    expect(connect).toBeDisabled();

    fireEvent.change(screen.getByLabelText("User OAuth Token"), {
      target: { value: "xoxb-1111111111-2222222222-abcdefabcdef" },
    });
    expect(screen.getByText(/A bot token \(xoxb-\) can.t read/)).toBeInTheDocument();
    expect(connect).toBeDisabled();

    fireEvent.change(screen.getByLabelText("User OAuth Token"), { target: { value: TOKEN } });
    fireEvent.click(connect);
    await waitFor(() => expect(native.connectSlack).toHaveBeenCalledWith(TOKEN));
    await waitFor(() => expect(onClose).toHaveBeenCalled());
  });

  it("says why Slack turned it down, and stays open", async () => {
    const onClose = vi.fn();
    native.connectSlack.mockRejectedValue(
      new Error("The token can't read your direct messages: add the im:history scope."),
    );
    wrap(<ConnectSlackDialog onClose={onClose} />);
    fireEvent.change(screen.getByLabelText("User OAuth Token"), { target: { value: TOKEN } });
    fireEvent.click(screen.getByRole("button", { name: "Connect and start reading" }));
    expect(await screen.findByText(/add the im:history scope/)).toBeInTheDocument();
    expect(onClose).not.toHaveBeenCalled();
  });

  it("takes a new token for a connected workspace", async () => {
    native.setSlackToken.mockResolvedValue({});
    wrap(<SlackToken sourceId="src-9" name="Acme (Slack)" />);
    fireEvent.click(screen.getByRole("button", { name: "New token" }));
    fireEvent.change(screen.getByLabelText("New Slack token for Acme (Slack)"), {
      target: { value: TOKEN },
    });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(native.setSlackToken).toHaveBeenCalledWith("src-9", TOKEN));
    expect(await screen.findByRole("button", { name: "New token" })).toBeInTheDocument();
  });
});
