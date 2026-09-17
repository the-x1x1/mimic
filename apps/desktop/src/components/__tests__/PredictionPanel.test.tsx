import { readFileSync } from "node:fs";
import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { SessionPhoto } from "@mimic/contracts";
import { sessionFixture } from "@mimic/test-fixtures";
import { PredictionPanel } from "../PredictionPanel";
import { ConfidenceBadge } from "../ConfidenceBadge";

const fixture = (): SessionPhoto =>
  SessionPhoto.parse(JSON.parse(readFileSync(sessionFixture("session_photo.json"), "utf8")));

describe("PredictionPanel", () => {
  it("shows the predicted Lightroom values, confidence reasons and the read-back mismatch", () => {
    const onReview = vi.fn();
    render(<PredictionPanel photo={fixture()} onReview={onReview} canApply onApplyOne={vi.fn()} />);
    expect(screen.getByText("Exposure")).toBeInTheDocument();
    expect(screen.getByText("+0.35 EV")).toBeInTheDocument();
    expect(screen.getByText("Exposure2012")).toBeInTheDocument();
    expect(screen.getByText("Similarity to training photos")).toBeInTheDocument();
    expect(screen.getByText("Camera Canon EOS R6 seen in training")).toBeInTheDocument();
    expect(screen.getByText("read-back mismatch")).toBeInTheDocument();
    expect(screen.getByText("5350")).toBeInTheDocument(); // sent
    expect(screen.getByText("5000")).toBeInTheDocument(); // observed
    fireEvent.click(screen.getByText("Reject"));
    expect(onReview).toHaveBeenCalledWith("rejected");
    fireEvent.click(screen.getByText("Looks right"));
    expect(onReview).toHaveBeenCalledWith("reviewed");
  });
  it("offers no decisions on applied predictions and un-reject on rejected ones", () => {
    const base = fixture();
    const applied = {
      ...base,
      prediction: { ...base.prediction!, status: "applied" as const },
      lastApply: null,
    };
    const { rerender } = render(<PredictionPanel photo={applied} onReview={vi.fn()} />);
    expect(screen.queryByText("Reject")).not.toBeInTheDocument();
    const rejected = { ...base, prediction: { ...base.prediction!, status: "rejected" as const } };
    rerender(<PredictionPanel photo={rejected} onReview={vi.fn()} />);
    expect(screen.getByText("Un-reject")).toBeInTheDocument();
  });
  it("disables per-photo apply with the given reason when Lightroom is offline", () => {
    render(
      <PredictionPanel
        photo={{ ...fixture(), lastApply: null }}
        onReview={vi.fn()}
        onApplyOne={vi.fn()}
        canApply={false}
        applyDisabledReason="Lightroom is not connected."
      />,
    );
    const btn = screen.getByText("Apply this photo").closest("button")!;
    expect(btn).toBeDisabled();
    expect(btn).toHaveAttribute("title", "Lightroom is not connected.");
  });
});

describe("ConfidenceBadge", () => {
  it("bands by the review thresholds and flags unfamiliar photos", () => {
    const p = fixture().prediction!;
    const { rerender } = render(<ConfidenceBadge prediction={p} />);
    expect(screen.getByText("83%")).toBeInTheDocument();
    rerender(<ConfidenceBadge prediction={{ ...p, confidence: 0.3 }} />);
    expect(screen.getByText("30%").className).toContain("danger");
    rerender(<ConfidenceBadge prediction={{ ...p, rawModelOutput: { ood: true } }} />);
    expect(screen.getByText("83% · unfamiliar")).toBeInTheDocument();
    rerender(<ConfidenceBadge prediction={null} />);
    expect(screen.getByText("not predicted")).toBeInTheDocument();
  });
});
