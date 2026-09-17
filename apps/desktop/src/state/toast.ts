import { create } from "zustand";

export interface Toast {
  id: number;
  title: string;
  body?: string;
  tone: "info" | "success" | "warning" | "danger";
  createdAt: number;
}

interface ToastState {
  toasts: Toast[];
  push: (t: Omit<Toast, "id" | "createdAt">) => void;
  dismiss: (id: number) => void;
}

let seq = 1;

export const useToasts = create<ToastState>((set) => ({
  toasts: [],
  push: (t) =>
    set((s) => ({ toasts: [...s.toasts.slice(-4), { ...t, id: seq++, createdAt: Date.now() }] })),
  dismiss: (id) => set((s) => ({ toasts: s.toasts.filter((t) => t.id !== id) })),
}));

export const toast = {
  info: (title: string, body?: string) => useToasts.getState().push({ title, body, tone: "info" }),
  success: (title: string, body?: string) =>
    useToasts.getState().push({ title, body, tone: "success" }),
  warning: (title: string, body?: string) =>
    useToasts.getState().push({ title, body, tone: "warning" }),
  danger: (title: string, body?: string) =>
    useToasts.getState().push({ title, body, tone: "danger" }),
};
