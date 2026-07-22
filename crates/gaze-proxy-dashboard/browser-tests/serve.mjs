/* Dev-only human verification runner.
 *
 * Starts the synthetic fixture server on an ephemeral loopback port and keeps
 * it alive so a reviewer can walk the rendered states manually. Prints the
 * origin and the synthetic launch token (a published test constant, not a
 * secret). Never used in production.
 */

import { startFixtureServer } from "./fixture-server.mjs";
import { LAUNCH_TOKEN } from "./fixtures.mjs";

const server = await startFixtureServer();
process.stdout.write("fixture server: " + server.origin + "\n");
process.stdout.write("launch token (synthetic test constant): " + LAUNCH_TOKEN + "\n");
process.stdout.write("switch fixture: POST " + server.origin + "/__fixture {\"id\":\"fx-...\"}\n");
