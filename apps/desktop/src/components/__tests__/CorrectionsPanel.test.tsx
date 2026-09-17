import { readFileSync } from "node:fs";
import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { CorrectionRow, StyleHealth } from "@mimic/contracts";
import { sessionFixture } from "@mimic/test-fixtures";
import { CorrectionsPanel } from "../CorrectionsPanel";

const health = (): StyleHealth =>
  StyleHealth.parse(JSON.parse(readFileSync(sessionFixture("style_health.json"), "utf8")));
const row = (): CorrectionRow =>
  CorrectionRow.parse(JSON.parse(readFileSync(sessionFixture("correction_row.json"), "utf8")));

describe("CorrectionsPanel", () => {
  it("shows measured No-Touch numbers, insights and pending corrections", () => {
    render(<CorrectionsPanel health={health()} corrections={[row()]} />);
    expect(screen.getAllByText("82%").length).toBeGreaterThan(0);
    expect(screen.getByText("65%")).toBeInTheDocument();
    expect(screen.getByText(/went from 65%/)).toBeInTheDocument();
    expect(screen.getByText("A0001.CR3")).toBeInTheDocument();
    expect(screen.getByText("pending")).toBeInTheDocument();
    expect(screen.getByText(/Exposure 0.35 → 0.85/)).toBeInTheDocument();
  });
  it("is an honest empty state before any sync", () => {
    const empty: StyleHealth = {
      ...health(),
      noTouch: [],
      activeNoTouchRate: null,
      correctionsTotal: 0,
      correctionsPendingTraining: 0,
      mostCorrected: [],
      insights: [],
    };
    render(<CorrectionsPanel health={empty} corrections={[]} />);
    expect(screen.getByText("No corrections synced yet")).toBeInTheDocument();
  });
});
