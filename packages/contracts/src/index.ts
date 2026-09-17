/**
 * @mimic/contracts — runtime-validated shapes shared by the React frontend and
 * the Tauri commands (which serialize mimic-core types with camelCase serde).
 *
 * Every IPC response is parsed through one of these schemas before the UI sees
 * it, so a native-side shape change fails loudly in development instead of
 * rendering undefined.
 */
export * from "./app";
export * from "./bridge";
export * from "./edit-mapping";
export * from "./jobs";
export * from "./library";
export * from "./lightroom";
export * from "./style";
export * from "./settings";
export * from "./updater";
