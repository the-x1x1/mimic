/**
 * @mimic/contracts — runtime-validated shapes shared by the React frontend and
 * the Tauri commands (which serialize mimic-core types with camelCase serde).
 *
 * Every IPC response is parsed through one of these schemas before the UI sees
 * it, so a native-side shape change fails loudly in development instead of
 * rendering undefined. The fixtures under `fixtures/contracts/` are written by
 * the Rust end-to-end test and parsed here, which is what keeps the two sides
 * from drifting.
 */
export * from "./app";
export * from "./compose";
export * from "./identity";
export * from "./jobs";
export * from "./privacy";
export * from "./providers";
export * from "./settings";
export * from "./sources";
export * from "./updater";
export * from "./voice";
