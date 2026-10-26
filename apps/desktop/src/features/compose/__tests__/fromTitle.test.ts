import { describe, expect, it } from "vitest";
import { personFromTitle } from "../fromTitle";

const people = [
  { id: "p1", name: "Ada Lovelace" },
  { id: "p2", name: "Bob Stone" },
  { id: "p3", name: "Bob Marley" },
  { id: "p4", name: "Al" },
];

describe("who a copied message is from, by its window's title", () => {
  it("finds someone named whole, or by a first name no one else has", () => {
    expect(personFromTitle("Ada Lovelace | Messenger", people)).toBe("p1");
    expect(personFromTitle("Chat | Ada Lovelace | Microsoft Teams", people)).toBe("p1");
    expect(personFromTitle("@ada - Discord", people)).toBe("p1");
    expect(personFromTitle("Bob Stone (DM) - Slack", people)).toBe("p2");
  });

  it("says nobody rather than guess", () => {
    expect(personFromTitle("Bob - Discord", people)).toBeNull();
    expect(personFromTitle("Adam Smith - WhatsApp", people)).toBeNull();
    expect(personFromTitle("WhatsApp", people)).toBeNull();
    expect(personFromTitle("Al - Signal", people)).toBeNull();
    expect(personFromTitle(null, people)).toBeNull();
    expect(personFromTitle("Ada Lovelace and Bob Stone - Group", people)).toBeNull();
  });
});
