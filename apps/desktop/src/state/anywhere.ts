import { create } from "zustand";

/**
 * The reply last copied out of Write after answering a message from anywhere
 * (0.10.0-alpha.35), until the user says whether it went. Mimic can't see it
 * being pasted or sent, so it asks rather than assuming: its draft stays
 * undecided until then. Held for this run only.
 */
export interface CopiedAway {
  draftId: string;
  /** What was copied: the draft as the user left it. */
  text: string;
  /** What Mimic wrote, to tell an edited reply from an unedited one. */
  generatedText: string;
}

interface AnywhereState {
  copied: CopiedAway | null;
  setCopied: (c: CopiedAway | null) => void;
}

export const useAnywhere = create<AnywhereState>((set) => ({
  copied: null,
  setCopied: (copied) => set({ copied }),
}));
