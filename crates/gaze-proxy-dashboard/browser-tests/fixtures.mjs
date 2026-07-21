/* Synthetic fixture data for the dashboard browser contract.
 *
 * Every value is synthetic: approved fixture PII only (alice@example.invalid,
 * Dr. Schmidt, +1-555-0142, +49 1555 0112233, <Email_1>). Payload sentinels
 * exist so tests can prove DOM-byte absence before reveal and after conceal.
 * The wire shapes model the closed typed view contract only; no provider
 * semantics, no raw bytes, no per-entry SSE byte counts or timing.
 */

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

function avail(value) { return { state: "Present", value }; }
function omitted(reason) { return { state: "Omitted", reason }; }

function stage(emissionId, domainState, measurements) {
  return { emissionId, domainState, measurements };
}

const M = (bytes, chunks) => avail({ bytes, chunks });

function smallStructure() {
  return avail({
    truncation: "Complete",
    nodes: [
      { ordinal: 0, type: "Object", depth: 0, children: 3 },
      { ordinal: 1, type: "String", depth: 1 },
      { ordinal: 2, type: "Array", depth: 1, children: 2 },
      { ordinal: 3, type: "Number", depth: 2 },
      { ordinal: 4, type: "Boolean", depth: 2 },
      { ordinal: 5, type: "Null", depth: 1 },
    ],
  });
}

function smallStream() {
  return avail({
    entries: [
      { ordinal: 0, kind: "MessageStart" },
      { ordinal: 1, kind: "ContentBlockStart", block: 0 },
      { ordinal: 2, kind: "ContentBlockDelta", delta: "Text", block: 0 },
      { ordinal: 3, kind: "ContentBlockDelta", delta: "Text", block: 0 },
      { ordinal: 4, kind: "ContentBlockDelta", delta: "Thinking", block: 0 },
      { ordinal: 5, kind: "ContentBlockDelta", delta: "Signature", block: 0 },
      { ordinal: 6, kind: "ContentBlockStop", block: 0 },
      { ordinal: 7, kind: "ContentBlockStart", block: 1 },
      { ordinal: 8, kind: "ContentBlockDelta", delta: "InputJson", block: 1 },
      { ordinal: 9, kind: "ContentBlockStop", block: 1 },
      { ordinal: 10, kind: "MessageDelta" },
      { ordinal: 11, kind: "MessageStop" },
    ],
  });
}

function fullDecisions() {
  return avail([
    { ordinal: 0, engine: "DeterministicRecognizer", outcome: "RanClean" },
    { ordinal: 1, engine: "Ner", outcome: "RanFindings", piiClass: "PersonName", detected: 2, pseudonymized: 2 },
    { ordinal: 2, engine: "NeuralSafetyNet", outcome: "SkippedNotApplicable" },
    { ordinal: 3, engine: "ResidualScan", outcome: "UnavailableFailClosed" },
  ]);
}

function baseEvent(overrides) {
  return Object.assign({
    logicalId: 1,
    ttlSeconds: 300,
    epoch: 1,
    route: "AnthropicMessages",
    profile: "AnthropicMessagesDirect",
    endpoint: "OsAssignedLoopback",
    port: "OsAssigned",
    statusClass: "Http2xx",
    durationBucket: "Under1s",
    retryCount: 0,
    stages: {
      providerRequest: stage(101, { kind: "Available" }, M(1421, 3)),
      providerResponse: stage(102, { kind: "Available" }, M(5230, 18)),
    },
    piiSummary: avail([
      { cls: "PersonName", detected: 2, pseudonymized: 2 },
      { cls: "Email", detected: 1, pseudonymized: 1 },
    ]),
    structure: smallStructure(),
    stream: smallStream(),
    decisions: fullDecisions(),
    attestation: avail("ExactHit"),
  }, overrides);
}

function runtime(overrides) {
  return Object.assign({
    lifecycle: "Running",
    captureTier: "ProviderVisible",
    ttlSeconds: 300,
    retained: 5,
    capacity: 64,
    epoch: 1,
  }, overrides);
}

const ZERO_COUNTERS = {
  ingressFull: 0, ingressClosed: 0, oversize: 0, ttlExpired: 0,
  capacityEvicted: 0, staleEpoch: 0, duplicateConflict: 0, followLag: 0,
};

const DROP_COUNTERS = {
  ingressFull: 7, ingressClosed: 2, oversize: 1, ttlExpired: 4,
  capacityEvicted: 3, staleEpoch: 2, duplicateConflict: 1, followLag: 0,
};

/* Default tier: five events covering representative profile, endpoint, and
 * port categories with no numeric/private value anywhere. */
function defaultEvents() {
  return [
    baseEvent({ logicalId: 5, endpoint: "OsAssignedLoopback", port: "OsAssigned" }),
    baseEvent({
      logicalId: 4,
      profile: "OpenAiCompatible",
      endpoint: "DynamicallyDiscoveredLoopback",
      port: "DynamicallyDiscovered",
      portChangeCount: 2,
      statusClass: "Http4xx",
      durationBucket: "Under10s",
      attestation: avail("NotApplicable"),
    }),
    baseEvent({
      logicalId: 3,
      profile: "GeminiCompatible",
      endpoint: "RevalidatedLoopback",
      port: "ChangedAndRevalidated",
      portChangeCount: 1,
      durationBucket: "Over10s",
      retryCount: 1,
      attestation: avail("MissExpired"),
    }),
    baseEvent({
      logicalId: 2,
      profile: "GenericLocalAnthropicCompatible",
      endpoint: "ConfiguredHttpsOrigin",
      port: "FixedConfigured",
      statusClass: "Http5xx",
      attestation: avail("UnavailableFailClosed"),
      decisions: avail([
        { ordinal: 0, engine: "DeterministicRecognizer", outcome: "RanClean" },
        { ordinal: 1, engine: "Ner", outcome: "TimeoutFailClosed" },
        { ordinal: 2, engine: "ResidualScan", outcome: "ErrorFailClosed" },
      ]),
    }),
    baseEvent({
      logicalId: 1,
      route: "Rejected",
      profile: "OtherReviewed",
      endpoint: "UnavailableFailClosed",
      port: "UnavailableFailClosed",
      statusClass: "Rejected",
      durationBucket: "Unknown",
      stages: {
        providerRequest: stage(11, { kind: "Omitted", reason: "ProjectionFailedClosed" }, omitted("ProjectionFailedClosed")),
      },
      piiSummary: omitted("ProjectionFailedClosed"),
      structure: omitted("ProjectionFailedClosed"),
      stream: omitted("NotApplicable"),
      decisions: omitted("ProjectionFailedClosed"),
      attestation: avail("AmbiguousFailClosed"),
    }),
  ];
}

function ownerStages(rawState, restoredState) {
  return {
    ownerRequest: stage(201, rawState, rawState.kind === "Concealed" ? M(1500, 1) : omitted("NotCapturedByPolicy")),
    providerRequest: stage(202, { kind: "Available" }, M(1421, 3)),
    providerResponse: stage(203, { kind: "Available" }, M(5230, 18)),
    ownerRestoredResponse: stage(204, restoredState, restoredState.kind === "Concealed" ? M(5300, 1) : omitted("NotCapturedByPolicy")),
  };
}

export const FIXTURES = {
  "fx-default": {
    snapshot: () => ({
      runtime: runtime({}),
      counters: ZERO_COUNTERS,
      queue: null,
      events: defaultEvents(),
    }),
    payloads: { providerVisible: PSEUDO_PAYLOAD },
  },

  "fx-owner-raw": {
    snapshot: () => ({
      runtime: runtime({ captureTier: "OwnerRaw" }),
      counters: ZERO_COUNTERS,
      queue: null,
      events: [
        baseEvent({
          logicalId: 7,
          stages: ownerStages({ kind: "Concealed" }, { kind: "NotCaptured" }),
          attestation: avail("MissNoRecord"),
        }),
        baseEvent({ logicalId: 6, attestation: avail("MissEvicted") }),
      ],
    }),
    payloads: { providerVisible: PSEUDO_PAYLOAD, ownerRaw: RAW_PAYLOAD },
  },

  "fx-owner-restored": {
    snapshot: () => ({
      runtime: runtime({ captureTier: "OwnerRestored" }),
      counters: ZERO_COUNTERS,
      queue: null,
      events: [
        baseEvent({
          logicalId: 9,
          stages: ownerStages({ kind: "NotCaptured" }, { kind: "Concealed" }),
          attestation: avail("InvalidatedPolicyOrRulepack"),
        }),
        baseEvent({ logicalId: 8, attestation: avail("InvalidatedContentBytes") }),
      ],
    }),
    payloads: { providerVisible: PSEUDO_PAYLOAD, ownerRestored: RESTORED_PAYLOAD },
  },

  "fx-owner-both": {
    snapshot: () => ({
      runtime: runtime({ captureTier: "Both", retained: 12 }),
      counters: DROP_COUNTERS,
      queue: null,
      events: [
        baseEvent({
          logicalId: 12,
          stages: ownerStages({ kind: "Concealed" }, { kind: "Concealed" }),
          attestation: avail("AmbiguousFailClosed"),
        }),
        baseEvent({ logicalId: 11, attestation: avail("InvalidatedSessionEpoch") }),
      ],
    }),
    payloads: { providerVisible: PSEUDO_PAYLOAD, ownerRaw: RAW_PAYLOAD, ownerRestored: RESTORED_PAYLOAD },
  },

  "fx-content-pv-omitted": {
    snapshot: () => ({
      runtime: runtime({}),
      counters: ZERO_COUNTERS,
      queue: null,
      events: [
        baseEvent({
          logicalId: 14,
          stages: {
            providerRequest: stage(301, { kind: "Available" }, M(1421, 3)),
            providerResponse: stage(302, { kind: "Omitted", reason: "LimitExceeded" }, omitted("LimitExceeded")),
          },
        }),
      ],
    }),
    payloads: { providerVisible: PSEUDO_PAYLOAD },
  },

  "fx-content-owner-omitted": {
    snapshot: () => ({
      runtime: runtime({ captureTier: "OwnerRaw" }),
      counters: ZERO_COUNTERS,
      queue: null,
      events: [
        baseEvent({
          logicalId: 15,
          stages: {
            ownerRequest: stage(311, { kind: "Omitted", reason: "SignedOrEncryptedSurface" }, omitted("SignedOrEncryptedSurface")),
            providerRequest: stage(312, { kind: "Available" }, M(1421, 3)),
            providerResponse: stage(313, { kind: "Available" }, M(5230, 18)),
          },
        }),
      ],
    }),
    payloads: { providerVisible: PSEUDO_PAYLOAD },
  },

  "fx-hostile": {
    snapshot: () => ({
      runtime: runtime({}),
      counters: ZERO_COUNTERS,
      queue: null,
      events: [baseEvent({ logicalId: 16 })],
    }),
    payloads: { providerVisible: HOSTILE_PAYLOAD },
  },

  "fx-structure-deep": {
    snapshot: () => {
      const nodes = [];
      for (let d = 0; d < 64; d++) {
        nodes.push({ ordinal: d, type: d % 2 === 0 ? "Object" : "Array", depth: d, children: d < 63 ? 1 : 0 });
      }
      return {
        runtime: runtime({}),
        counters: ZERO_COUNTERS,
        queue: null,
        events: [baseEvent({ logicalId: 20, structure: avail({ truncation: "DepthLimit", nodes }) })],
      };
    },
    payloads: { providerVisible: PSEUDO_PAYLOAD },
  },

  "fx-structure-wide": {
    snapshot: () => {
      const nodes = [{ ordinal: 0, type: "Object", depth: 0, children: 512 }];
      for (let i = 1; i <= 512; i++) {
        nodes.push({ ordinal: i, type: "String", depth: 1 });
      }
      return {
        runtime: runtime({}),
        counters: ZERO_COUNTERS,
        queue: null,
        events: [baseEvent({ logicalId: 21, structure: avail({ truncation: "NodeLimit", nodes }) })],
      };
    },
    payloads: { providerVisible: PSEUDO_PAYLOAD },
  },

  "fx-structure-malformed": {
    snapshot: () => ({
      runtime: runtime({}),
      counters: ZERO_COUNTERS,
      queue: null,
      events: [baseEvent({ logicalId: 22, structure: omitted("MalformedFailClosed") })],
    }),
    payloads: { providerVisible: PSEUDO_PAYLOAD },
  },

  "fx-sse-10k": {
    snapshot: () => {
      const entries = [];
      entries.push({ ordinal: 0, kind: "MessageStart" });
      for (let i = 1; i < 9998; i++) {
        const mod = i % 5;
        if (mod === 1) { entries.push({ ordinal: i, kind: "ContentBlockStart", block: Math.floor(i / 100) }); }
        else if (mod === 2 || mod === 3) { entries.push({ ordinal: i, kind: "ContentBlockDelta", delta: "Text", block: Math.floor(i / 100) }); }
        else if (mod === 4) { entries.push({ ordinal: i, kind: "ContentBlockStop", block: Math.floor(i / 100) }); }
        else { entries.push({ ordinal: i, kind: "Ping" }); }
      }
      entries.push({ ordinal: 9998, kind: "MessageDelta" });
      entries.push({ ordinal: 9999, kind: "MessageStop" });
      return {
        runtime: runtime({}),
        counters: ZERO_COUNTERS,
        queue: null,
        events: [baseEvent({ logicalId: 23, stream: avail({ entries }) })],
      };
    },
    payloads: { providerVisible: PSEUDO_PAYLOAD },
  },

  "fx-drops": {
    snapshot: () => ({
      runtime: runtime({ retained: 60, capacity: 64 }),
      counters: DROP_COUNTERS,
      queue: null,
      events: defaultEvents(),
    }),
    payloads: { providerVisible: PSEUDO_PAYLOAD },
  },

  "fx-zero": {
    snapshot: () => ({
      runtime: runtime({ retained: 0 }),
      counters: ZERO_COUNTERS,
      queue: null,
      events: [],
    }),
    payloads: {},
  },

  "fx-disconnected": {
    snapshot: () => ({
      runtime: { lifecycle: "Disabled", disabledCode: "ChildExit" },
      counters: ZERO_COUNTERS,
      queue: null,
      events: [],
    }),
    payloads: {},
  },

  "fx-shutdown": {
    snapshot: () => ({
      runtime: { lifecycle: "ShutdownPurged" },
      counters: ZERO_COUNTERS,
      queue: null,
      events: [],
    }),
    payloads: {},
  },

  "fx-purged": {
    snapshot: () => ({
      runtime: runtime({ lifecycle: "Purged", retained: 0 }),
      counters: ZERO_COUNTERS,
      queue: null,
      events: [],
    }),
    payloads: {},
  },
};
