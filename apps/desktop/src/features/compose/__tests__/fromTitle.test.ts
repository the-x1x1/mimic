import { describe, expect, it } from "vitest";
import { channelFromTitle, personFromTitle } from "../fromTitle";

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

describe("what kind of conversation a copied message is from, by its window's title", () => {
  it("knows chat apps, texts and mail by the app's own part of the title", () => {
    expect(channelFromTitle("WhatsApp")).toBe("chat");
    expect(channelFromTitle("Ada Lovelace | Messenger")).toBe("chat");
    expect(channelFromTitle("@ada - Discord")).toBe("chat");
    expect(channelFromTitle("#general | Acme - Discord")).toBe("chat");
    expect(channelFromTitle("Chat | Ada Lovelace | Microsoft Teams")).toBe("chat");
    expect(channelFromTitle("Bob Stone (DM) - Acme - Slack")).toBe("chat");
    expect(channelFromTitle("Phone Link")).toBe("sms");
    expect(channelFromTitle("(2) Messages for web - Google Chrome")).toBe("sms");
    expect(channelFromTitle("Inbox - ada@example.com - Outlook")).toBe("email");
    expect(channelFromTitle("Inbox (3) - ada@gmail.com - Gmail - Google Chrome")).toBe("email");
    expect(channelFromTitle("Inbox - Mozilla Thunderbird")).toBe("email");
    expect(
      channelFromTitle("Mail - Ada Lovelace - Outlook - Personal - Microsoft\u200b Edge"),
    ).toBe("email");
  });

  it("reads the app, not a word in the subject, and says nothing it doesn't know", () => {
    expect(channelFromTitle("Your WhatsApp code - ada@gmail.com - Gmail")).toBe("email");
    expect(channelFromTitle("WhatsApp - ada@gmail.com - Gmail")).toBe("email");
    expect(channelFromTitle("Slack - Message (HTML)")).toBe("email");
    expect(channelFromTitle("Signal processing notes.docx - Word")).toBeNull();
    expect(channelFromTitle("Untitled - Notepad")).toBeNull();
    expect(channelFromTitle("")).toBeNull();
    expect(channelFromTitle(null)).toBeNull();
  });
});
