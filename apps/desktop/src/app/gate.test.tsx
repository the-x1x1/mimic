import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { App } from "./App";

describe("App outside Tauri", () => {
  it("renders an honest explanation instead of a white screen", () => {
    render(<App />);
    expect(screen.getByText(/Open Mimic from the desktop app/)).toBeInTheDocument();
  });
});
