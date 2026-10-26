import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { ServerTranscript } from "../ServerTranscript";

describe("what the server said", () => {
  it("shows nothing when nothing was kept", () => {
    const { container } = render(<ServerTranscript lines={[]} />);
    expect(container).toBeEmptyDOMElement();
  });

  it("is there on request, and can be copied whole", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
    const lines = [
      "imap connecting to imap.example.com:993 over TLS",
      "imap > m1 LOGIN [the username and password]",
      "imap < m1 NO [AUTHENTICATIONFAILED] Invalid credentials",
    ];
    render(<ServerTranscript lines={lines} />);
    expect(screen.getByText("What the server said")).toBeInTheDocument();
    expect(screen.getByText(/m1 NO \[AUTHENTICATIONFAILED\]/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Copy it" }));
    await waitFor(() => expect(writeText).toHaveBeenCalledWith(lines.join("\n")));
    expect(await screen.findByRole("button", { name: "Copied" })).toBeInTheDocument();
  });
});
