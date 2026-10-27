import { create } from "zustand";

/** Someone whose conversations Find is to look in. */
export interface FindWith {
  id: string;
  name: string;
}

interface FindWithState {
  /**
   * Asked for from People (0.10.0-alpha.102), until Find opens and takes it:
   * Find then looks only in the conversations this person is in.
   */
  pending: FindWith | null;
  ask: (who: FindWith | null) => void;
}

export const useFindWith = create<FindWithState>((set) => ({
  pending: null,
  ask: (pending) => set({ pending }),
}));
