import { readFileSync } from "node:fs";
import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { SessionDetail, SessionPhoto } from "@mimic/contracts";
import { sessionFixture } from "@mimic/test-fixtures";
import { GroupsPanel } from "../GroupsPanel";

const detail = SessionDetail.parse(
  JSON.parse(readFileSync(sessionFixture("session_detail.json"), "utf8")),
);
const photo = SessionPhoto.parse(
  JSON.parse(readFileSync(sessionFixture("session_photo.json"), "utf8")),
);
const photos: SessionPhoto[] = [
  photo,
  {
    ...photo,
    asset: { ...photo.asset, id: "asset-2", fileName: "A0002.CR3" },
    clusterId: "cluster-2",
  },
];

describe("GroupsPanel", () => {
  it("shows per-group confidence, attention counts and the reference photo", () => {
    render(
      <GroupsPanel
        clusters={detail.clusters}
        stats={detail.groupStats}
        photos={photos}
        selected={[]}
        onEdit={vi.fn()}
        onFilter={vi.fn()}
        activeFilter="all"
      />,
    );
    expect(screen.getByText("Ceremony")).toBeInTheDocument();
    expect(screen.getByText("81%")).toBeInTheDocument();
    expect(screen.getByText("9")).toBeInTheDocument(); // 6 + 1 + 2
    expect(screen.getByText("A0001.CR3")).toBeInTheDocument();
    expect(screen.getByText("edited")).toBeInTheDocument();
  });
  it("rename, reference and move emit the right edits", () => {
    const onEdit = vi.fn();
    render(
      <GroupsPanel
        clusters={detail.clusters}
        stats={detail.groupStats}
        photos={photos}
        selected={["asset-2"]}
        onEdit={onEdit}
        onFilter={vi.fn()}
        activeFilter="all"
      />,
    );
    fireEvent.click(screen.getByLabelText("Rename Group 2"));
    fireEvent.change(screen.getByLabelText("Group name"), { target: { value: "Reception" } });
    fireEvent.click(screen.getByLabelText("Save name"));
    expect(onEdit).toHaveBeenCalledWith({
      kind: "rename",
      clusterId: "cluster-2",
      label: "Reception",
    });
    // asset-2 belongs to cluster-2, so only that group offers "Use selected".
    fireEvent.click(screen.getByText("Use selected"));
    expect(onEdit).toHaveBeenCalledWith({
      kind: "setReference",
      clusterId: "cluster-2",
      assetId: "asset-2",
    });
    fireEvent.click(screen.getByText("Move 1 here"));
    expect(onEdit).toHaveBeenCalledWith({
      kind: "move",
      assetIds: ["asset-2"],
      into: "cluster-1",
      label: null,
    });
    fireEvent.change(screen.getByLabelText("New group name"), { target: { value: "Details" } });
    fireEvent.click(screen.getByText(/Split 1 selected/));
    expect(onEdit).toHaveBeenCalledWith({
      kind: "move",
      assetIds: ["asset-2"],
      into: null,
      label: "Details",
    });
  });
  it("merge is a two-step choice", () => {
    const onEdit = vi.fn();
    render(
      <GroupsPanel
        clusters={detail.clusters}
        stats={detail.groupStats}
        photos={photos}
        selected={[]}
        onEdit={onEdit}
        onFilter={vi.fn()}
        activeFilter="all"
      />,
    );
    fireEvent.click(screen.getByLabelText("Merge Group 2 into another group"));
    fireEvent.click(screen.getByText("merge here"));
    expect(onEdit).toHaveBeenCalledWith({ kind: "merge", into: "cluster-1", from: "cluster-2" });
  });
});
