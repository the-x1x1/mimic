import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { DataQualityReport } from "@mimic/contracts";
import { DataQualityPanel } from "../DataQualityPanel";

const base: DataQualityReport = {
  libraryId: "l1",
  assetsFound: 120,
  validPairs: 0,
  missingEdits: 120,
  featuresComputed: 120,
  lightroomConnectedPairs: 0,
  sidecarOnlyPairs: 0,
  acrHeavyEditCount: 3,
  localEditCount: 0,
  failedSidecars: 0,
  cameras: [
    { label: "Canon R5", count: 110 },
    { label: "Sony A7 IV", count: 10 },
  ],
  captureDays: [{ label: "2025-04-12", count: 120 }],
  recommendation: {
    level: "insufficient",
    headline: "Not enough edited examples yet (0 of 30 minimum)",
    detail: "Add more edited photos.",
  },
  warnings: ["Some Lightroom edits are stored in an ACR sidecar and cannot be fully read offline."],
};

describe("DataQualityPanel", () => {
  it("shows the recommendation, counts and warnings without inventing metrics", () => {
    render(<DataQualityPanel report={base} />);
    expect(screen.getByText(/Not enough edited examples yet/)).toBeInTheDocument();
    expect(screen.getByText("insufficient")).toBeInTheDocument();
    expect(screen.getByText("0% of photos have edits")).toBeInTheDocument();
    expect(screen.getAllByText(/ACR sidecar/).length).toBeGreaterThan(0);
    expect(screen.getByText("Canon R5")).toBeInTheDocument();
    expect(screen.queryByText(/No-Touch/)).not.toBeInTheDocument();
  });
  it("compact mode hides secondary metrics", () => {
    render(
      <DataQualityPanel
        report={{
          ...base,
          validPairs: 60,
          missingEdits: 60,
          recommendation: { level: "minimal", headline: "60 edited examples", detail: "" },
        }}
        compact
      />,
    );
    expect(screen.getByText("minimal")).toBeInTheDocument();
    expect(screen.queryByText("Sidecar-only")).not.toBeInTheDocument();
  });
});
