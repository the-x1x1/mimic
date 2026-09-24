import { useState, type ReactNode } from "react";
import * as Dialog from "@radix-ui/react-dialog";
import { fireEvent, render, screen, within } from "@testing-library/react";
import { MemoryRouter, Route, Routes } from "react-router-dom";
import { describe, expect, it, vi } from "vitest";
import { AppShell } from "../AppShell";
import { SECTIONS, sectionOf } from "../sections";

// The shell alone: what it shows around the drawer, not the screens in it.
vi.mock("@/features/replies/RepliesPage", async () => {
  const { Link } = await import("react-router-dom");
  return {
    RepliesPage: () => (
      <>
        <p>the replies</p>
        <Link to="/write">Write something new</Link>
      </>
    ),
  };
});
vi.mock("@/hooks/useSystem", () => ({ useSystemStatus: () => ({ data: undefined }) }));
vi.mock("@/hooks/useCompose", () => ({
  useProviderState: () => ({ data: undefined }),
  useProviderHealth: () => ({ data: undefined }),
}));
vi.mock("../JobTray", () => ({ JobTray: () => null }));
vi.mock("../Toaster", () => ({ Toaster: () => null }));

function renderAt(path: string, people: ReactNode = <p>the people screen</p>) {
  render(
    <MemoryRouter initialEntries={[path]}>
      <Routes>
        <Route element={<AppShell />}>
          <Route path="/" element={null} />
          <Route path="/write" element={<textarea aria-label="What you want to say" autoFocus />} />
          <Route path="/find" element={<p>the find screen</p>} />
          <Route path="/people" element={people} />
          <Route path="/voice" element={<p>the voice screen</p>} />
          <Route path="/sources" element={<p>the mail screen</p>} />
          <Route path="/settings" element={<p>the settings screen</p>} />
        </Route>
      </Routes>
    </MemoryRouter>,
  );
}

describe("everything in the drawer can be reached", () => {
  it("puts every section in the bar, and each opens its screen in the drawer", () => {
    renderAt("/");
    const bar = screen.getByRole("navigation", { name: "Everything else" });
    expect(
      within(bar)
        .getAllByRole("link")
        .map((l) => l.textContent),
    ).toEqual(["Find", "Your mail", "People", "How you write", "Settings"]);
    expect(screen.queryByRole("dialog")).toBeNull();

    const screens: Record<string, string> = {
      Find: "the find screen",
      "Your mail": "the mail screen",
      People: "the people screen",
      "How you write": "the voice screen",
      Settings: "the settings screen",
    };
    for (const [label, text] of Object.entries(screens)) {
      fireEvent.click(within(bar).getByRole("link", { name: label }));
      const drawer = screen.getByRole("dialog", { name: label });
      expect(within(drawer).getByText(text)).toBeInTheDocument();
      // The same row in the drawer, with the open one marked.
      const inside = within(drawer).getByRole("navigation", { name: "Sections" });
      expect(within(inside).getByRole("link", { name: label })).toHaveAttribute(
        "aria-current",
        "page",
      );
    }
    expect(screen.getByText("the replies")).toBeInTheDocument();
  });

  it("goes from one section to another inside the drawer, and back to the replies", () => {
    renderAt("/settings");
    const drawer = screen.getByRole("dialog", { name: "Settings" });
    fireEvent.click(
      within(within(drawer).getByRole("navigation", { name: "Sections" })).getByRole("link", {
        name: "People",
      }),
    );
    expect(screen.getByText("the people screen")).toBeInTheDocument();
    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("names the drawer after what is open in it", () => {
    expect(sectionOf("/people")).toBe("People");
    expect(sectionOf("/settings")).toBe("Settings");
    expect(sectionOf("/write")).toBeNull();
    expect(SECTIONS.map((s) => s.to)).toEqual([
      "/find",
      "/sources",
      "/people",
      "/voice",
      "/settings",
    ]);
    expect(sectionOf("/find")).toBe("Find");
    renderAt("/write");
    expect(screen.getByRole("dialog", { name: "Write something new" })).toBeInTheDocument();
  });

  it("leaves the drawer open when Escape closes a dialog inside it", () => {
    function DialogInside() {
      const [open, setOpen] = useState(true);
      return open ? (
        <Dialog.Root open onOpenChange={(o) => !o && setOpen(false)}>
          <Dialog.Portal>
            <Dialog.Content>
              <Dialog.Title>Conversations with Ada</Dialog.Title>
              <Dialog.Description>Every conversation.</Dialog.Description>
            </Dialog.Content>
          </Dialog.Portal>
        </Dialog.Root>
      ) : (
        <p>the dialog closed</p>
      );
    }
    renderAt("/people", <DialogInside />);
    expect(screen.getByRole("dialog", { name: "Conversations with Ada" })).toBeInTheDocument();
    fireEvent.keyDown(document.activeElement ?? document.body, { key: "Escape" });
    expect(screen.getByText("the dialog closed")).toBeInTheDocument();
    expect(screen.getByRole("dialog", { name: "People" })).toBeInTheDocument();
  });

  it("takes focus into the drawer and gives it back when it closes", () => {
    renderAt("/");
    const link = within(screen.getByRole("navigation", { name: "Everything else" })).getByRole(
      "link",
      { name: "People" },
    );
    link.focus();
    fireEvent.click(link);
    expect(document.activeElement).toBe(screen.getByRole("dialog", { name: "People" }));
    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(document.activeElement).toBe(link);
  });

  it("gives focus back to what opened Write, though Write takes focus for its own box", () => {
    renderAt("/");
    const link = screen.getByRole("link", { name: "Write something new" });
    link.focus();
    fireEvent.click(link);
    expect(document.activeElement).toBe(
      screen.getByRole("textbox", { name: "What you want to say" }),
    );
    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(document.activeElement).toBe(link);
  });
});
