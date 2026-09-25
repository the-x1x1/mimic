import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { OnboardingFlow } from "../OnboardingFlow";

const native = vi.hoisted(() => ({
  onboardingState: vi.fn(),
  userIdentity: vi.fn(),
  setUserIdentity: vi.fn(),
  completeOnboarding: vi.fn(),
  jobs: vi.fn(),
}));
vi.mock("@/lib/ipc", () => ({ ipc: native }));
vi.mock("../ModelStep", () => ({ ModelStep: () => <p>Model setup</p> }));

function open() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  render(
    <QueryClientProvider client={client}>
      <OnboardingFlow />
    </QueryClientProvider>,
  );
}

beforeEach(() => {
  vi.resetAllMocks();
  native.onboardingState.mockResolvedValue({
    completed: false,
    hasIdentity: false,
    hasSource: false,
    hasOwnMessages: false,
    hasVoiceProfile: false,
  });
  native.userIdentity.mockResolvedValue(null);
  native.jobs.mockResolvedValue([]);
  native.completeOnboarding.mockResolvedValue(undefined);
});

describe("first-run welcome", () => {
  it("opens the workspace without requiring identity or model setup", async () => {
    open();
    fireEvent.click(await screen.findByRole("button", { name: "Explore first" }));
    await waitFor(() => expect(native.completeOnboarding).toHaveBeenCalledOnce());
    expect(native.setUserIdentity).not.toHaveBeenCalled();
    expect(screen.queryByLabelText("Your name")).toBeNull();
    expect(screen.queryByText("Model setup")).toBeNull();
  });

  it("takes a name-only identity to file import without an email address", async () => {
    native.setUserIdentity.mockResolvedValue({ id: "me", displayName: "Sam", identifiers: [] });
    open();
    fireEvent.click(await screen.findByRole("button", { name: "Import my messages" }));
    fireEvent.change(await screen.findByLabelText("Your name"), { target: { value: "Sam" } });
    fireEvent.click(screen.getByRole("button", { name: "Continue" }));
    expect(await screen.findByText("Choose messages to learn from")).toBeInTheDocument();
    expect(native.setUserIdentity).toHaveBeenCalledWith("Sam", expect.anything());
  });

  it("shows a completion error and allows retry", async () => {
    native.completeOnboarding.mockRejectedValueOnce(new Error("Could not save setup"));
    open();
    fireEvent.click(await screen.findByRole("button", { name: "Explore first" }));
    expect(await screen.findByText("Could not save setup")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Explore first" }));
    await waitFor(() => expect(native.completeOnboarding).toHaveBeenCalledTimes(2));
  });
});
