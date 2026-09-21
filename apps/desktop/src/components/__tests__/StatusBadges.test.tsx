import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { ProviderInfo } from "@mimic/contracts";
import { ProviderBadge } from "../StatusBadges";

const provider = (over: Partial<ProviderInfo> = {}): ProviderInfo => ({
  id: "local",
  displayName: "Local model",
  local: true,
  model: "llama3.1:8b",
  description: "",
  requiresCredential: false,
  ...over,
});

describe("ProviderBadge", () => {
  it("says plainly when a provider keeps messages on the machine", () => {
    render(<ProviderBadge provider={provider()} />);
    expect(screen.getByText("Local model")).toBeInTheDocument();
  });

  it("says plainly when it does not", () => {
    render(<ProviderBadge provider={provider({ local: false, displayName: "Claude" })} />);
    expect(screen.getByText("Sends to Claude")).toBeInTheDocument();
  });

  it("stops claiming a model is there when it does not answer", () => {
    render(
      <ProviderBadge
        provider={provider()}
        health={{ reachable: false, error: "connection refused" }}
      />,
    );
    expect(screen.getByText("Local model · not answering")).toBeInTheDocument();
  });

  it("says nothing new until the check has actually run", () => {
    render(<ProviderBadge provider={provider()} health={undefined} />);
    expect(screen.getByText("Local model")).toBeInTheDocument();
  });

  it("does not imply a provider exists when none is configured", () => {
    render(<ProviderBadge provider={undefined} />);
    expect(screen.getByText("No model configured")).toBeInTheDocument();
  });
});
