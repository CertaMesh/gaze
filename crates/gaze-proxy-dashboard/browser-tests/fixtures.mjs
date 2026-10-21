/* Fixture data for the dashboard browser contract.
 *
 * Snapshot/follow bodies are NOT written here. They are the exact output of the
 * Rust snapshot encoder (src/browser_fixtures.rs), generated into
 * encoder-fixtures.json by `cargo test -p gaze-proxy-dashboard browser_fixtures`
 * and verified against the committed encoder-fixtures.sha256 before use, so the
 * browser contract runs against the wire the child really emits.
 *
 * Payload bodies (served only by the payload/reveal routes) are synthetic:
 * approved fixture PII only (alice@example.invalid, Dr. Schmidt, +1-555-0142,
 * +49 1555 0112233, <Email_1>). Payload sentinels exist so tests can prove
 * DOM-byte absence before reveal and after conceal.
 */

import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

export const LAUNCH_TOKEN = "CanaryLaunchTok_000000000000000000000000000"; // 43 base64url chars
export const WRONG_TOKEN = "WrongLaunchToken000000000000000000000000000"; // 43 chars, rejected

export const RAW_SENTINEL = "OWNER-RAW-SENTINEL-7Q4";
export const RESTORED_SENTINEL = "OWNER-RESTORED-SENTINEL-9K2";
export const PSEUDO_SENTINEL = "PSEUDO-PAYLOAD-SENTINEL-5B8";

const PSEUDO_PAYLOAD =
  PSEUDO_SENTINEL + "\n" +
  '{"message":"Please contact <PersonName_1> at <Email_1> or <Phone_1>."}';

const RAW_PAYLOAD =
  RAW_SENTINEL + "\n" +
  '{"message":"Please contact Dr. Schmidt at alice@example.invalid or +1-555-0142."}';

const RESTORED_PAYLOAD =
  RESTORED_SENTINEL + "\n" +
  '{"message":"Restored for owner: Dr. Schmidt, alice@example.invalid, +49 1555 0112233."}';

/* Hostile text fixture: markup, prototype-pollution-shaped keys, bidi and
 * control characters, exercised through the pseudonymized payload path. */
const HOSTILE_PAYLOAD =
  PSEUDO_SENTINEL + "\n" +
  '<img src=x onerror=alert(1)><script>alert(2)</script>' +
  '{"__proto__":{"polluted":true},"constructor":"x"}' +
  "bidi:‮gnihsihp‬ ctrl: zw:​ end";

const HERE = dirname(fileURLToPath(import.meta.url));

function loadEncoderFixtures() {
  const regenerate = "run `cargo test -p gaze-proxy-dashboard browser_fixtures` first";
  let raw;
  try {
    raw = readFileSync(join(HERE, "encoder-fixtures.json"));
  } catch (_err) {
    throw new Error("encoder-fixtures.json is missing; " + regenerate);
  }
  const expected = readFileSync(join(HERE, "encoder-fixtures.sha256"), "utf-8").trim();
  const actual = createHash("sha256").update(raw).digest("hex");
  if (actual !== expected) {
    throw new Error("encoder-fixtures.json is not the blessed encoder output; " + regenerate);
  }
  return JSON.parse(raw.toString("utf-8"));
}

const ENCODED = loadEncoderFixtures();

/* A fresh deep copy per request, exactly as the encoder rendered it. */
function encoded(id) {
  if (!Object.prototype.hasOwnProperty.call(ENCODED, id)) {
    throw new Error("no encoder fixture " + id);
  }
  const body = JSON.stringify(ENCODED[id]);
  return () => JSON.parse(body);
}

/* Terminal lifecycle frames the current child encoder never emits: app.js keeps
 * fail-closed handling for them, so the harness still drives those branches.
 * They carry no event, stage, or payload-domain field. */
const TERMINAL_FRAMES = {
  "fx-shutdown": { runtime: { lifecycle: "ShutdownPurged" }, events: [] },
  "fx-purged": { runtime: { lifecycle: "Purged" }, events: [] },
};

function terminal(id) {
  const body = JSON.stringify(TERMINAL_FRAMES[id]);
  return () => JSON.parse(body);
}

const PV = { providerVisible: PSEUDO_PAYLOAD };

export const FIXTURES = {
  "fx-default": { snapshot: encoded("fx-default"), payloads: PV },
  "fx-owner-raw": {
    snapshot: encoded("fx-owner-raw"),
    payloads: { providerVisible: PSEUDO_PAYLOAD, ownerRaw: RAW_PAYLOAD },
  },
  "fx-owner-restored": {
    snapshot: encoded("fx-owner-restored"),
    payloads: { providerVisible: PSEUDO_PAYLOAD, ownerRestored: RESTORED_PAYLOAD },
  },
  "fx-owner-both": {
    snapshot: encoded("fx-owner-both"),
    payloads: { providerVisible: PSEUDO_PAYLOAD, ownerRaw: RAW_PAYLOAD, ownerRestored: RESTORED_PAYLOAD },
  },
  "fx-content-pv-omitted": { snapshot: encoded("fx-content-pv-omitted"), payloads: PV },
  "fx-content-owner-omitted": { snapshot: encoded("fx-content-owner-omitted"), payloads: PV },
  "fx-hostile": { snapshot: encoded("fx-hostile"), payloads: { providerVisible: HOSTILE_PAYLOAD } },
  "fx-structure-deep": { snapshot: encoded("fx-structure-deep"), payloads: PV },
  "fx-structure-wide": { snapshot: encoded("fx-structure-wide"), payloads: PV },
  "fx-structure-malformed": { snapshot: encoded("fx-structure-malformed"), payloads: PV },
  "fx-sse-10k": { snapshot: encoded("fx-sse-10k"), payloads: PV },
  "fx-drops": { snapshot: encoded("fx-drops"), payloads: PV },
  "fx-zero": { snapshot: encoded("fx-zero"), payloads: {} },
  "fx-disconnected": { snapshot: encoded("fx-disconnected"), payloads: {} },
  "fx-shutdown": { snapshot: terminal("fx-shutdown"), payloads: {} },
  "fx-purged": { snapshot: terminal("fx-purged"), payloads: {} },
};
