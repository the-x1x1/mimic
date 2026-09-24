import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { EncoderView } from "@mimic/contracts";
import { MeaningSection } from "../MeaningSection";

// The real hooks run; only the native calls are replaced.
const native = vi.hoisted(() => ({
  encoder: vi.fn(),
  downloadEncoder: vi.fn(),
  jobs: vi.fn(),
}));
vi.mock("@/lib/ipc", () => ({ ipc: native }));

const offered = {
  id: "all-minilm-l6-v2",
  name: "all-MiniLM-L6-v2",
  description: "A small English sentence encoder.",
  bytes: 23_513_036,
  license: "Apache-2.0",
  homepage: "https://huggingface.co/sentence-transformers/all-MiniLM-L6-v2",
};

const view = (over: Partial<EncoderView>): EncoderView => ({
  offered,
  downloaded: false,
  inUse: false,
  reason: null,
  wanted: 0,
  done: 0,
  ...over,
});

function renderIt(v: EncoderView) {
  native.encoder.mockResolvedValue(v);
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <MeaningSection />
    </QueryClientProvider>,
  );
}

describe("finding past replies by meaning", () => {
  beforeEach(() => {
    for (const f of Object.values(native)) f.mockReset();
    native.jobs.mockResolvedValue([]);
    native.downloadEncoder.mockResolvedValue({ id: "j1" });
  });

  it("offers the download, with its size, and starts it when asked", async () => {
    renderIt(view({}));
    const button = await screen.findByRole("button", { name: "Download all-MiniLM-L6-v2 (24 MB)" });
    expect(
      screen.getByText(/reading your messages for meaning sends nothing anywhere/),
    ).toBeInTheDocument();
    fireEvent.click(button);
    await waitFor(() => expect(native.downloadEncoder).toHaveBeenCalled());
  });

  it("shows a download under way", async () => {
    native.jobs.mockResolvedValue([
      {
        id: "j1",
        type: "download_encoder",
        status: "running",
        progressCurrent: 5_000_000,
        progressTotal: 23_513_036,
      },
    ]);
    renderIt(view({}));
    expect(await screen.findByRole("button", { name: "Downloading…" })).toBeDisabled();
    expect(screen.getByRole("progressbar", { name: "Downloading" })).toHaveAttribute(
      "aria-valuenow",
      "5000000",
    );
  });

  it("says how much it has read once in use", async () => {
    renderIt(view({ downloaded: true, inUse: true, wanted: 1200, done: 300 }));
    expect(await screen.findByText(/300 messages of 1,200 read\./)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Download/ })).toBeNull();
    expect(screen.getByRole("progressbar")).toHaveAttribute("aria-valuenow", "300");
  });

  it("says why a downloaded encoder is not being used", async () => {
    renderIt(
      view({
        downloaded: true,
        reason: "all-MiniLM-L6-v2: model.onnx does not match the SHA-256 its manifest pins",
      }),
    );
    expect(
      await screen.findByText(/isn’t using it: all-MiniLM-L6-v2: model\.onnx does not match/),
    ).toBeInTheDocument();
    // What no longer matches can be fetched again.
    fireEvent.click(screen.getByRole("button", { name: "Download it again" }));
    await waitFor(() => expect(native.downloadEncoder).toHaveBeenCalled());
  });

  it("does not offer the same files again when downloading would not help", async () => {
    renderIt(
      view({
        downloaded: true,
        reason: "all-MiniLM-L6-v2: could not be loaded (this build cannot run an encoder)",
      }),
    );
    expect(await screen.findByText(/isn’t using it: .*could not be loaded/)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Download/ })).toBeNull();
  });

  it("shows nothing in a build that offers no encoder", async () => {
    renderIt(view({ offered: null }));
    await waitFor(() => expect(native.encoder).toHaveBeenCalled());
    expect(screen.queryByText(/by meaning/)).toBeNull();
  });
});
