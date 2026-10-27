import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { ChosenWay, LearningOverview } from "@mimic/contracts";
import { ChosenWays } from "../ChosenWays";

function overview(ways: ChosenWay[], choicesConsidered: number): LearningOverview {
  return { draftsConsidered: 0, patterns: [], choicesConsidered, ways, minAgreeing: 3, notes: [] };
}

function way(over: Partial<ChosenWay>): ChosenWay {
  return {
    adjustment: "shorter",
    participantId: null,
    participantName: null,
    chosen: 3,
    against: 0,
    share: 1,
    holds: true,
    meanChange: -0.4,
    summary:
      "You chose the shorter way over my first drafts 3 times, so I write shorter from the start.",
    ...over,
  };
}

describe("the ways you chose over my first drafts", () => {
  it("says nothing before a choice has been made", () => {
    const { container } = render(<ChosenWays overview={overview([], 0)} />);
    expect(container).toBeEmptyDOMElement();
  });

  it("lists what holds, then what is still forming, in my words", () => {
    render(
      <ChosenWays
        overview={overview(
          [
            way({}),
            way({
              adjustment: "moreCasual",
              participantId: "p1",
              participantName: "Ada",
              chosen: 1,
              holds: false,
              meanChange: null,
              summary:
                "You chose the more casual way over my first draft to Ada once. 2 times more and I'll take the hint.",
            }),
          ],
          4,
        )}
      />,
    );
    expect(screen.getByRole("region", { name: "Ways you chose" })).toBeInTheDocument();
    expect(screen.getByText(/From 4 choices between ways of saying a draft/)).toBeInTheDocument();
    const items = screen.getAllByRole("listitem").map((li) => li.textContent);
    expect(items).toEqual([
      "You chose the shorter way over my first drafts 3 times, so I write shorter from the start.",
      "You chose the more casual way over my first draft to Ada once. 2 times more and I'll take the hint.",
    ]);
  });

  it("says so when nothing chosen holds yet", () => {
    render(
      <ChosenWays
        overview={overview(
          [
            way({
              chosen: 2,
              holds: false,
              summary: "You chose the shorter way over my first drafts 2 times.",
            }),
          ],
          2,
        )}
      />,
    );
    expect(
      screen.getByText(/No way you’ve chosen has added up to a habit yet/),
    ).toBeInTheDocument();
  });

  it("says the first draft was kept when it was sent every time", () => {
    render(<ChosenWays overview={overview([], 2)} />);
    expect(
      screen.getByText(/You sent my first draft over the other ways each time/),
    ).toBeInTheDocument();
  });
});
