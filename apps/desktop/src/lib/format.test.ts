import { describe, expect, it } from "vitest";
import { formatBytes, formatRelative, formatShutter, truncateMiddle } from "./format";

describe("format helpers", () => {
  it("bytes", () => {
    expect(formatBytes(0)).toBe("0 B");
    expect(formatBytes(1536)).toBe("1.5 KB");
    expect(formatBytes(3 * 1024 ** 3)).toBe("3.0 GB");
  });
  it("relative time", () => {
    const now = new Date("2026-09-16T12:00:00Z");
    expect(formatRelative(null, now)).toBe("never");
    expect(formatRelative("2026-09-16T11:59:50Z", now)).toBe("just now");
    expect(formatRelative("2026-09-16T11:30:00Z", now)).toBe("30 min ago");
    expect(formatRelative("2026-09-15T12:00:00Z", now)).toBe("1 d ago");
  });
  it("shutter and truncation", () => {
    expect(formatShutter(1 / 200)).toBe("1/200");
    expect(formatShutter(2.5)).toBe("2.5s");
    expect(truncateMiddle("abcdefghijklmnop", 9)).toBe("abcd…mnop");
  });
});
