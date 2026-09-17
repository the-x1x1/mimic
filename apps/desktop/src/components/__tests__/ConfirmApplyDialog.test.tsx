import { readFileSync } from "node:fs";
import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { ApplyPreflight } from "@mimic/contracts";
import { sessionFixture } from "@mimic/test-fixtures";
import { ConfirmApplyDialog } from "@/features/sessions/ConfirmApplyDialog";

const refused = (): ApplyPreflight =>
  ApplyPreflight.parse(
    JSON.parse(readFileSync(sessionFixture("apply_preflight.refused.json"), "utf8")),
  );

describe("ConfirmApplyDialog", () => {
  it("explains the safety steps and blocks the confirm button when preflight refuses", () => {
    const onConfirm = vi.fn();
    render(
      <ConfirmApplyDialog
        open
        onOpenChange={vi.fn()}
        preflight={refused()}
        photoCount={12}
        onConfirm={onConfirm}
        busy={false}
        scopeLabel="all pending predictions"
      />,
    );
    expect(screen.getByText(/develop snapshot is created/)).toBeInTheDocument();
    expect(screen.getByText(/read back and compared/)).toBeInTheDocument();
    expect(screen.getByText(/different Lightroom capability set/)).toBeInTheDocument();
    expect(screen.getByText("2 stale")).toBeInTheDocument();
    const btn = screen.getByText("Apply 12 photos").closest("button")!;
    expect(btn).toBeDisabled();
    fireEvent.click(btn);
    expect(onConfirm).not.toHaveBeenCalled();
  });
  it("enables confirm only when preflight is ok", () => {
    const onConfirm = vi.fn();
    const ok: ApplyPreflight = {
      ...refused(),
      ok: true,
      blockers: [],
      staleCount: 0,
      candidateCount: 30,
    };
    render(
      <ConfirmApplyDialog
        open
        onOpenChange={vi.fn()}
        preflight={ok}
        photoCount={30}
        onConfirm={onConfirm}
        busy={false}
        scopeLabel="all pending predictions"
      />,
    );
    expect(screen.getByText(/2 batches/)).toBeInTheDocument();
    fireEvent.click(screen.getByText("Apply 30 photos"));
    expect(onConfirm).toHaveBeenCalled();
  });
});
