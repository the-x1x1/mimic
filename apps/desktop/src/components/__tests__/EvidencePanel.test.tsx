import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { GenerationContext, VoiceMetrics } from "@mimic/contracts";
import { EvidencePanel } from "../EvidencePanel";

const emptyMetrics: VoiceMetrics = {
  sampleSize: 0,
  measurable: false,
  avgWordsPerMessage: null,
  medianWordsPerMessage: null,
  p90WordsPerMessage: null,
  avgSentencesPerMessage: null,
  multiParagraphRate: null,
  terminalPeriodRate: null,
  questionRate: null,
  exclamationRate: null,
  ellipsisRate: null,
  emojiRate: null,
  lowercaseStartRate: null,
  allLowercaseRate: null,
  contractionsPer100Words: null,
  greetingRate: null,
  signOffRate: null,
  topGreetings: [],
  topSignOffs: [],
  topPhrases: [],
  medianResponseSeconds: null,
};

function context(over: Partial<GenerationContext> = {}): GenerationContext {
  return {
    participant: null,
    channel: "email",
    voice: { layers: [], overrides: [], examples: [] },
    effective: emptyMetrics,
    examples: [],
    transcript: [],
    evidence: [],
    ...over,
  } as GenerationContext;
}

describe("EvidencePanel", () => {
  it("shows the evidence statements verbatim", () => {
    render(
      <EvidencePanel
        loading={false}
        context={context({ evidence: ["412 of your messages to this person"] })}
      />,
    );
    expect(screen.getByText("412 of your messages to this person")).toBeInTheDocument();
  });

  it("shows no habits at all when nothing was measured", () => {
    render(<EvidencePanel loading={false} context={context()} />);
    expect(screen.queryByText(/How you write, measured/)).not.toBeInTheDocument();
  });

  it("renders a measured zero as a fact, not as an absence", () => {
    render(
      <EvidencePanel
        loading={false}
        context={context({
          effective: { ...emptyMetrics, measurable: true, sampleSize: 200, emojiRate: 0 },
        })}
      />,
    );
    expect(screen.getByText("0% of messages contain an emoji")).toBeInTheDocument();
  });

  it("lists the user's own messages that are being used, with the reason", () => {
    render(
      <EvidencePanel
        loading={false}
        context={context({
          examples: [
            {
              replyMessageId: "m1",
              reply: "yeah sending it over",
              incoming: "can you send the deck",
              participantId: null,
              channel: "email",
              sentAt: null,
              score: 0.8,
              reason: "similar wording: deck, send",
            },
          ],
        })}
      />,
    );
    expect(screen.getByText("yeah sending it over")).toBeInTheDocument();
    expect(screen.getByText("can you send the deck")).toBeInTheDocument();
    expect(screen.getByText("similar wording: deck, send")).toBeInTheDocument();
  });

  it("renders nothing rather than a skeleton when there is no context", () => {
    const { container } = render(<EvidencePanel loading={false} context={undefined} />);
    expect(container).toBeEmptyDOMElement();
  });
});
