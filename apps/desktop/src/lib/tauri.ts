/** True when running inside the Tauri webview (vs. Vite in a browser or vitest). */
export function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}
