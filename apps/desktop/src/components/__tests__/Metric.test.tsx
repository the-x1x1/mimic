import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { Metric } from "@mimic/ui";

describe("a figure on its own", () => {
  it("sets a number large and a phrase in its place as words", () => {
    render(
      <>
        <Metric label="Your messages analyzed" value="21" />
        <Metric label="Last analyzed" value="34 min ago" />
        <Metric label="Drafts you sent unchanged" value="not measured yet" />
        <Metric label="People with their own profile" value={0} />
      </>,
    );
    expect(screen.getByText("21")).not.toHaveClass("ui-metric__value--words");
    expect(screen.getByText("34 min ago")).not.toHaveClass("ui-metric__value--words");
    expect(screen.getByText("0")).not.toHaveClass("ui-metric__value--words");
    expect(screen.getByText("not measured yet")).toHaveClass("ui-metric__value--words");
  });
});
