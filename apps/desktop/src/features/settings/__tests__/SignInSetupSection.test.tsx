import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { SignInSetup } from "@mimic/contracts";
import { SignInSetupSection } from "../SignInSetupSection";

// The real hooks run; only the native calls are replaced.
const native = vi.hoisted(() => ({
  signInSetup: vi.fn(),
  saveSignInSetup: vi.fn(),
  forgetSignInSetup: vi.fn(),
  mailSignInAvailable: vi.fn(),
}));
vi.mock("@/lib/ipc", () => ({ ipc: native }));

const none: SignInSetup = {
  microsoft: { clientId: null, own: false, builtIn: false },
  google: { clientId: null, own: false, builtIn: true },
};
const MS = "0f5c1a2b-3c4d-4e5f-8a9b-0c1d2e3f4a5b";

function renderIt() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <SignInSetupSection />
    </QueryClientProvider>,
  );
}

describe("registering Mimic with Microsoft and Google", () => {
  beforeEach(() => {
    for (const f of Object.values(native)) f.mockReset();
    native.signInSetup.mockResolvedValue(none);
  });

  it("says which registration each signs in with", async () => {
    renderIt();
    const microsoft = await screen.findByRole("region", { name: "Signing in with Microsoft" });
    expect(
      within(microsoft).getByText(/Not set up: this copy of me was built without a registration/),
    ).toBeInTheDocument();
    const google = screen.getByRole("region", { name: "Signing in with Google" });
    expect(within(google).getByText(/as this copy's own registration/)).toBeInTheDocument();
  });

  it("keeps a Microsoft registration pasted in, and goes back to none when it is forgotten", async () => {
    native.saveSignInSetup.mockResolvedValue({
      ...none,
      microsoft: { clientId: MS, own: true, builtIn: false },
    });
    native.forgetSignInSetup.mockResolvedValue(none);
    renderIt();
    const microsoft = await screen.findByRole("region", { name: "Signing in with Microsoft" });
    fireEvent.click(within(microsoft).getByRole("button", { name: "Register me with Microsoft" }));
    expect(within(microsoft).getByText(/http:\/\/localhost/)).toBeInTheDocument();
    fireEvent.change(within(microsoft).getByLabelText("Application (client) ID"), {
      target: { value: ` ${MS} ` },
    });
    fireEvent.click(within(microsoft).getByRole("button", { name: "Keep it" }));
    await waitFor(() =>
      expect(native.saveSignInSetup).toHaveBeenCalledWith("microsoft", ` ${MS} `, null),
    );
    expect(
      await within(microsoft).findByText(new RegExp(`your own registration, ${MS}`)),
    ).toBeInTheDocument();
    fireEvent.click(within(microsoft).getByRole("button", { name: "Forget it" }));
    await waitFor(() => expect(native.forgetSignInSetup).toHaveBeenCalledWith("microsoft"));
    expect(await within(microsoft).findByText(/Not set up/)).toBeInTheDocument();
  });

  it("asks for Google's secret with its client ID, and says why one is refused", async () => {
    native.saveSignInSetup.mockRejectedValue(
      new Error(
        "That isn't a Google client ID. Google Cloud shows it ending in .apps.googleusercontent.com.",
      ),
    );
    renderIt();
    const google = await screen.findByRole("region", { name: "Signing in with Google" });
    fireEvent.click(within(google).getByRole("button", { name: "Register me with Google" }));
    expect(within(google).getByText(/every seven days/)).toBeInTheDocument();
    fireEvent.change(within(google).getByLabelText("Client ID"), { target: { value: "abc" } });
    const keep = within(google).getByRole("button", { name: "Keep it" });
    expect(keep).toBeDisabled();
    fireEvent.change(within(google).getByLabelText("Client secret"), {
      target: { value: "GOCSPX-x" },
    });
    expect(keep).toBeEnabled();
    fireEvent.click(keep);
    await waitFor(() =>
      expect(native.saveSignInSetup).toHaveBeenCalledWith("google", "abc", "GOCSPX-x"),
    );
    expect(await within(google).findByText(/isn't a Google client ID/)).toBeInTheDocument();
  });
});
