import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { SessionPhoto, type PredictionStatus } from "@mimic/contracts";
import { sessionFixture } from "@mimic/test-fixtures";
import { reviewQueue } from "./reviewQueue";

const base = SessionPhoto.parse(
  JSON.parse(readFileSync(sessionFixture("session_photo.json"), "utf8")),
);
const withConf = (
  id: string,
  confidence: number,
  status: PredictionStatus = "pending",
): SessionPhoto => ({
  ...base,
  asset: { ...base.asset, id },
  lastApply: null,
  prediction: { ...base.prediction!, id: `p-${id}`, confidence, status },
});

describe("reviewQueue", () => {
  const photos = [
    withConf("a", 0.95),
    withConf("b", 0.4),
    withConf("c", 0.7, "rejected"),
    { ...withConf("d", 0.9), prediction: null },
    base, // 0.83 but verify_failed apply
  ];
  it("attention = below threshold or failed apply, never rejected/unpredicted", () => {
    expect(reviewQueue(photos, "attention", 0.6).map((p) => p.asset.id)).toEqual(["b", "asset-1"]);
    expect(reviewQueue(photos, "attention", 0.96).map((p) => p.asset.id)).toEqual([
      "a",
      "b",
      "asset-1",
    ]);
  });
  it("pending = pending/reviewed predictions; all = anything predicted", () => {
    expect(reviewQueue(photos, "pending", 0.6).map((p) => p.asset.id)).toEqual([
      "a",
      "b",
      "asset-1",
    ]);
    expect(reviewQueue(photos, "all", 0.6)).toHaveLength(4);
  });
});
