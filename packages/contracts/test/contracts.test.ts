import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { bridgeFixture, expectedFixture } from "@mimic/test-fixtures";
import {
  ApplyBatchResult,
  CapabilityMatrix,
  CommandEnvelope,
  CommandResultBody,
  CommandType,
  EventsBody,
  HandshakeRequest,
  HandshakeResponse,
  LatestJson,
  controlByLightroomKey,
  controlLabel,
  editMapping,
  nextCheckDelayMs,
  primaryError,
  controlMae,
  evaluationSet,
} from "../src";

const load = (p: string) => JSON.parse(readFileSync(p, "utf8"));

describe("bridge fixtures match the TypeScript contracts", () => {
  it("handshake", () => {
    const req = HandshakeRequest.parse(load(bridgeFixture("handshake.request.json")));
    expect(req.protocolVersion).toBe(1);
    expect(req.capabilities.developSettingKeys.length).toBeGreaterThan(50);
    HandshakeResponse.parse(load(bridgeFixture("handshake.response.json")));
  });
  it("commands and results", () => {
    const cmd = CommandEnvelope.parse(load(bridgeFixture("get_develop_settings.command.json")));
    expect(cmd.commandType).toBe("get_develop_settings");
    CommandResultBody.parse(load(bridgeFixture("get_develop_settings.result.json")));
    const partial = CommandResultBody.parse(
      load(bridgeFixture("apply_settings_as_plugin_preset.result.partial_failure.json")),
    );
    const batch = ApplyBatchResult.parse(partial.result);
    expect(batch.items.map((i) => i.status)).toEqual(["applied", "failed"]);
    const err = CommandResultBody.parse(load(bridgeFixture("command_error.result.json")));
    expect(err.error?.code).toBe("catalog_write_denied");
    EventsBody.parse(load(bridgeFixture("event.selection_changed.json")));
  });
  it("command list equals the spec set", () => {
    expect(CommandType.options).toEqual([
      "ping",
      "get_catalog_info",
      "get_selected_photos",
      "get_photo_metadata",
      "get_develop_settings",
      "create_before_snapshot",
      "apply_settings_as_plugin_preset",
      "read_back_develop_settings",
      "collect_correction_state",
      "get_capabilities",
    ]);
  });
});

describe("edit mapping contract", () => {
  it("loads, has unique keys and known families", () => {
    expect(editMapping.mappingVersion).toBe("edit_mapping_v1");
    const keys = editMapping.controls.flatMap((c) => c.lightroomKeys);
    expect(new Set(keys).size).toBe(keys.length);
    for (const c of editMapping.controls) {
      expect(editMapping.families[c.family]).toBeDefined();
      expect(c.canonical.startsWith(`${c.family}.`)).toBe(true);
    }
    expect(controlByLightroomKey.get("Exposure2012")?.canonical).toBe("tone.exposure");
    expect(controlLabel("tone.exposure")).toBe("Exposure");
    expect(controlLabel("hsl.saturation.orange")).toBe("Saturation Orange");
  });
  it("every raw fixture key is either mapped, metadata, local, heavy or intentionally unknown", () => {
    const raw = load(expectedFixture("modern_masks_unknown.raw.json")).rawSettings as Record<
      string,
      unknown
    >;
    const metadataKeys = new Set(Object.values(editMapping.lightroomMetadataKeys).flat());
    const unknown = Object.keys(raw).filter(
      (k) =>
        !controlByLightroomKey.has(k) &&
        !metadataKeys.has(k) &&
        !editMapping.localCorrectionKeyPrefixes.some((p) => k.startsWith(p)) &&
        !editMapping.heavyEditKeyPrefixes.some((p) => k.startsWith(p)),
    );
    expect(unknown).toEqual(["SomeNewSlider2027"]);
  });
  it("capability matrix schema accepts a matrix shaped like the Rust output", () => {
    const m = CapabilityMatrix.parse({
      schemaVersion: "cap_abc",
      lightroomVersion: "14.3",
      pluginVersion: "0.1.0-alpha.1",
      probeHadPhoto: true,
      canApply: true,
      canSnapshot: true,
      canRead: true,
      controls: [
        {
          canonical: "tone.exposure",
          family: "tone",
          status: "supported",
          lightroomKey: "Exposure2012",
          reason: "ok",
        },
      ],
      familySummary: {
        tone: { label: "Basic Tone", supported: 1, observedNotWritable: 0, unsupported: 0 },
      },
      localEdits: "unsupported",
      masks: "unsupported",
    });
    expect(m.controls[0]?.status).toBe("supported");
  });
});

describe("updater contracts", () => {
  it("latest.json requires signature + url per platform", () => {
    expect(() =>
      LatestJson.parse({
        version: "0.1.1",
        platforms: { "windows-x86_64": { signature: "", url: "https://x/y" } },
      }),
    ).toThrow();
    LatestJson.parse({
      version: "0.1.1",
      platforms: {
        "windows-x86_64": {
          signature: "sig",
          url: "https://github.com/the-x1x1/mimic/releases/download/v0.1.1/Mimic_0.1.1_x64-setup.exe",
        },
      },
    });
  });
  it("check delay is 6h with bounded jitter", () => {
    const six = 6 * 3600 * 1000;
    expect(nextCheckDelayMs(() => 0.5)).toBe(six);
    expect(nextCheckDelayMs(() => 0)).toBe(six - 20 * 60 * 1000);
    expect(nextCheckDelayMs(() => 1)).toBe(six + 20 * 60 * 1000);
  });
});

describe("training metrics helpers", () => {
  const metrics = {
    holdout: {
      n: 20,
      hybrid: { overall: { nMae: 0.04 }, perControl: { "tone.exposure": { mae: 0.21 } } },
      global_median: { overall: { nMae: 0.09 } },
    },
    validation: { n: 18, hybrid: { overall: { nMae: 0.05 } } },
  };
  it("prefers holdout and falls back to validation", () => {
    expect(primaryError(metrics)).toBe(0.04);
    expect(evaluationSet(metrics)).toBe("holdout");
    expect(primaryError({ holdout: { n: 0 }, validation: metrics.validation })).toBe(0.05);
    expect(primaryError({})).toBeNull();
    expect(primaryError(null)).toBeNull();
    expect(controlMae(metrics, "hybrid", "tone.exposure")).toBe(0.21);
    expect(controlMae(metrics, "hybrid", "tone.contrast")).toBeNull();
  });
});
