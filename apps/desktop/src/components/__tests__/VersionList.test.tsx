import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { ModelVersion } from "@mimic/contracts";
import { VersionList } from "../VersionList";

const base: ModelVersion = {
  id: "v1",
  styleProfileId: "s",
  semanticVersion: "1.0.0",
  modelType: "hybrid_knn_residual",
  featureSchemaVersion: "features_v1",
  editSchemaVersion: "1.0",
  trainingSetId: "t",
  trainingConfig: {},
  metrics: {
    holdout: {
      n: 12,
      hybrid: { overall: { nMae: 0.0412 }, perControl: { "tone.exposure": { mae: 0.18 } } },
    },
  },
  artifactManifest: { beatsBaselines: { evaluated: true, beatsGlobalMedian: true } },
  createdAt: "2026-09-16T10:00:00Z",
  status: "ready",
  isActive: true,
};

describe("VersionList", () => {
  it("shows real holdout numbers and no activate button on the active version", () => {
    render(<VersionList versions={[base]} onActivate={vi.fn()} onArchive={vi.fn()} />);
    expect(screen.getByText("0.0412")).toBeInTheDocument();
    expect(screen.getByText("0.18 EV")).toBeInTheDocument();
    expect(screen.getByText("holdout")).toBeInTheDocument();
    expect(screen.queryByText("Activate")).not.toBeInTheDocument();
  });
  it("offers activate/archive for inactive ready versions and shows failures honestly", () => {
    const failed: ModelVersion = {
      ...base,
      id: "v2",
      semanticVersion: "1.1.0",
      status: "failed",
      isActive: false,
      metrics: { error: { message: "12 usable pairs; at least 30 are needed" } },
      artifactManifest: {},
    };
    const inactive: ModelVersion = { ...base, id: "v3", semanticVersion: "1.2.0", isActive: false };
    render(<VersionList versions={[failed, inactive]} onActivate={vi.fn()} onArchive={vi.fn()} />);
    expect(screen.getByText(/at least 30 are needed/)).toBeInTheDocument();
    expect(screen.getAllByText("Activate")).toHaveLength(1);
    expect(screen.getAllByText("—").length).toBeGreaterThan(0);
  });
});
