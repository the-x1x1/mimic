import { fileURLToPath } from "node:url";
import { dirname, resolve } from "node:path";

const here = dirname(fileURLToPath(import.meta.url));
export const fixturesRoot = resolve(here, "../../fixtures");

/**
 * Read models produced by `crates/mimic-core/tests/pipeline_e2e.rs`. Both sides
 * parse these, so a shape change on one side that is not made on the other
 * fails a test rather than rendering `undefined`.
 */
export const contractFixture = (name: string) => resolve(fixturesRoot, "contracts", name);

/** Conversation exports used by the importer's tests. */
export const importFixture = (name: string) => resolve(fixturesRoot, "import", name);
