import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { ModelVersion, NoTouchStats } from "@mimic/contracts";
import { VersionCompare } from "../VersionCompare";

const mk = (
  id: string,
  semver: string,
  nMae: number,
  tone: number,
  active: boolean,
): ModelVersion => ({
  id,
  styleProfileId: "s",
  semanticVersion: semver,
  modelType: "hybrid_knn_residual",
  featureSchemaVersion: "features_v1",
  editSchemaVersion: "1.0",
  trainingSetId: "t",
  trainingConfig: {},
  metrics: {
    holdout: {
      n: 10,
      hybrid: { overall: { nMae }, perFamily: { tone: { n: 10, nMae: tone, p90: 0.1 } } },
    },
  },
  artifactManifest: {},
  createdAt: "2026-09-17T10:00:00Z",
  status: "ready",
  isActive: active,
});

describe("VersionCompare", () => {
  it("compares overall and per-family error and shows measured No-Touch only", () => {
    const nt: NoTouchStats[] = [
      {
        modelVersionId: "a",
        semanticVersion: "1.0.0",
        appliedChecked: 10,
        corrected: 5,
        untouched: 5,
        rate: 0.5,
      },
    ];
    render(
      <VersionCompare
        versions={[mk("a", "1.0.0", 0.05, 0.04, true), mk("b", "1.1.0", 0.04, 0.045, false)]}
        noTouch={nt}
      />,
    );
    expect(screen.getByText("0.0500")).toBeInTheDocument();
    expect(screen.getAllByText("0.0400")).toHaveLength(2); // b overall, a tone
    expect(screen.getByText("0.0450")).toBeInTheDocument();
    expect(screen.getByText("Basic Tone")).toBeInTheDocument();
    expect(screen.getByText("50%")).toBeInTheDocument();
    // overall: b better; tone family: a better
    const better = screen
      .getAllByRole("cell")
      .filter((c) => /^v1\.[01]\.0$/.test(c.textContent ?? ""));
    expect(better.map((c) => c.textContent)).toEqual(["v1.1.0", "v1.0.0"]);
  });
  it("asks for a second version when only one exists", () => {
    render(<VersionCompare versions={[mk("a", "1.0.0", 0.05, 0.04, true)]} noTouch={[]} />);
    expect(screen.getByText(/Train a second version/)).toBeInTheDocument();
  });
});
