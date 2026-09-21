import { describe, expect, it } from "vitest";
import { countOf, describeDownload, formatBytes, formatRelative, truncateMiddle } from "./format";

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
  it("counts and truncation", () => {
    expect(countOf(1, "message")).toBe("1 message");
    expect(countOf(0, "message")).toBe("0 messages");
    expect(countOf(2400, "person", "people")).toBe("2,400 people");
    expect(truncateMiddle("abcdefghijklmnop", 9)).toBe("abcd…mnop");
  });
});

describe("a download in progress", () => {
  it("does not report a percentage of a size nobody has yet", () => {
    expect(describeDownload(0, 0)).toBe("Starting the download…");
    expect(describeDownload(4_000_000, 0)).toBe("Starting the download…");
  });

  it("counts in gigabytes once the host says how big it is", () => {
    expect(describeDownload(640_000_000, 2_100_000_000)).toBe("0.6 GB of 2.1 GB");
  });
});
