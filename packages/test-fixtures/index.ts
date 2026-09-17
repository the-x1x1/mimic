import { fileURLToPath } from "node:url";
import { dirname, resolve } from "node:path";

const here = dirname(fileURLToPath(import.meta.url));
export const fixturesRoot = resolve(here, "../../fixtures");
export const bridgeFixture = (name: string) => resolve(fixturesRoot, "bridge", name);
export const expectedFixture = (name: string) => resolve(fixturesRoot, "expected", name);
export const sessionFixture = (name: string) => resolve(fixturesRoot, "sessions", name);
