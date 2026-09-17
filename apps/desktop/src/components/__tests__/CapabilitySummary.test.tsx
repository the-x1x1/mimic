import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { CapabilityMatrix } from "@mimic/contracts";
import { CapabilitySummary } from "../CapabilitySummary";

const matrix: CapabilityMatrix = {
  schemaVersion: "cap_0123456789abcdef",
  lightroomVersion: "14.3",
  pluginVersion: "0.1.0-alpha.1",
  probeHadPhoto: false,
  canApply: false,
  canSnapshot: true,
  canRead: true,
  controls: [],
  familySummary: {
    tone: { label: "Basic Tone", supported: 0, observedNotWritable: 6, unsupported: 0 },
  },
  localEdits: "unsupported",
  masks: "unsupported — mask/AI data is not readable or writable by Mimic 0.x",
};

describe("CapabilitySummary", () => {
  it("never claims apply support the probe did not grant", () => {
    render(<CapabilitySummary matrix={matrix} live />);
    expect(screen.getByText("apply no")).toBeInTheDocument();
    expect(screen.getByText("read yes")).toBeInTheDocument();
    expect(
      screen.getByText(/No photo was selected during the capability probe/),
    ).toBeInTheDocument();
    expect(screen.getByText(/mask\/AI data is not readable/)).toBeInTheDocument();
  });
});
