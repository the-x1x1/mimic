import type { ReactNode } from "react";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ConnectTelegramDialog } from "../ConnectTelegramDialog";

// The real hooks run; only the native calls are replaced.
const native = vi.hoisted(() => ({
  telegramApp: vi.fn(),
  startTelegramSignIn: vi.fn(),
  telegramSignInCode: vi.fn(),
  telegramSignInPassword: vi.fn(),
  cancelTelegramSignIn: vi.fn(),
}));
vi.mock("@/lib/ipc", () => ({ ipc: native }));

function wrap(node: ReactNode) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(<QueryClientProvider client={client}>{node}</QueryClientProvider>);
}

const step = (s: "code" | "password", hint: string | null = null, wrong = false) => ({
  step: s,
  hint,
  wrong,
  source: null,
});

describe("connecting Telegram", () => {
  beforeEach(() => {
    for (const f of Object.values(native)) f.mockReset();
    native.cancelTelegramSignIn.mockResolvedValue(undefined);
  });

  it("asks for the app once, then the phone, the code and the password", async () => {
    const onClose = vi.fn();
    native.telegramApp.mockResolvedValue({ saved: false, apiId: null });
    native.startTelegramSignIn.mockResolvedValue(step("code"));
    native.telegramSignInCode.mockResolvedValue(step("password", "the usual"));
    native.telegramSignInPassword
      .mockResolvedValueOnce(step("password", "the usual", true))
      .mockResolvedValueOnce({
        step: "done",
        hint: null,
        wrong: false,
        source: { name: "C Haynes (Telegram)" },
      });
    wrap(<ConnectTelegramDialog onClose={onClose} />);
    expect(await screen.findByText(/API development tools/)).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("App api_id"), { target: { value: "1234567" } });
    fireEvent.change(screen.getByLabelText("App api_hash"), {
      target: { value: "0123456789abcdef0123456789abcdef" },
    });
    fireEvent.change(screen.getByLabelText(/Your phone number/), {
      target: { value: "+1 808 555 0123" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Send me a code" }));
    await waitFor(() =>
      expect(native.startTelegramSignIn).toHaveBeenCalledWith(
        "+1 808 555 0123",
        "1234567",
        "0123456789abcdef0123456789abcdef",
      ),
    );

    fireEvent.change(await screen.findByLabelText("The code Telegram sent you"), {
      target: { value: "12345" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Sign in" }));
    expect(await screen.findByText(/its hint: the usual/)).toBeInTheDocument();

    fireEvent.change(screen.getByLabelText("Password"), { target: { value: "not it" } });
    fireEvent.click(screen.getByRole("button", { name: "Sign in" }));
    expect(await screen.findByText("That password isn’t right.")).toBeInTheDocument();
    expect(screen.getByLabelText("Password")).toHaveValue("");

    fireEvent.change(screen.getByLabelText("Password"), { target: { value: "right one" } });
    fireEvent.click(screen.getByRole("button", { name: "Sign in" }));
    await waitFor(() => expect(onClose).toHaveBeenCalled());
    expect(native.telegramSignInPassword).toHaveBeenLastCalledWith("right one");
  });

  it("uses the app kept before, and stops the sign-in when closed", async () => {
    const onClose = vi.fn();
    native.telegramApp.mockResolvedValue({ saved: true, apiId: 1234567 });
    native.startTelegramSignIn.mockResolvedValue(step("code"));
    wrap(<ConnectTelegramDialog onClose={onClose} />);
    expect(await screen.findByText(/api_id 1234567/)).toBeInTheDocument();
    expect(screen.queryByLabelText("App api_hash")).toBeNull();
    fireEvent.change(screen.getByLabelText(/Your phone number/), {
      target: { value: "+18085550123" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Send me a code" }));
    await waitFor(() =>
      expect(native.startTelegramSignIn).toHaveBeenCalledWith("+18085550123", null, null),
    );
    await screen.findByLabelText("The code Telegram sent you");
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(native.cancelTelegramSignIn).toHaveBeenCalled();
    expect(onClose).toHaveBeenCalled();
  });

  it("says what Telegram said about a wrong code, and lets it be tried again", async () => {
    native.telegramApp.mockResolvedValue({ saved: true, apiId: 1234567 });
    native.startTelegramSignIn.mockResolvedValue(step("code"));
    native.telegramSignInCode.mockRejectedValue(
      Object.assign(new Error("That code isn't right, or it has expired. Ask for a new one."), {
        code: "bad_code",
      }),
    );
    wrap(<ConnectTelegramDialog onClose={vi.fn()} />);
    fireEvent.change(await screen.findByLabelText(/Your phone number/), {
      target: { value: "+18085550123" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Send me a code" }));
    fireEvent.change(await screen.findByLabelText("The code Telegram sent you"), {
      target: { value: "1" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Sign in" }));
    expect(await screen.findByText(/That code isn't right/)).toBeInTheDocument();
    expect(screen.getByLabelText("The code Telegram sent you")).toBeInTheDocument();
  });
});
