/* Gaze dashboard application script.
 *
 * Rendering contract: createElement + textContent + fixed attributes +
 * finite clamped numeric dimensions only. No HTML sinks, no eval, no
 * data-derived URLs/CSS, no storage, no cookies, no service worker, no
 * external resource. Every rendered value is either a closed-code label
 * from the tables below, a bounded count, or payload text rendered as a
 * sanitized text node. No success/affirmative styling exists.
 */
"use strict";
(() => {
  /* ================= closed-code label tables =================
   * Wire codes are the closed #4732 vocabulary. Unknown codes never render
   * their spelling; they fall closed to the UNKNOWN label. */

  const UNKNOWN_LABEL = "UNKNOWN";

  const PROFILE_LABELS = {
    AnthropicMessagesDirect: "ANTHROPIC MESSAGES DIRECT",
    OpenAiCompatible: "OPENAI COMPATIBLE",
    GeminiCompatible: "GEMINI COMPATIBLE",
    GenericLocalAnthropicCompatible: "GENERIC LOCAL ANTHROPIC COMPATIBLE",
    OtherReviewed: "OTHER REVIEWED",
  };

  const ENDPOINT_LABELS = {
    FixedHttpsOrigin: "FIXED HTTPS ORIGIN",
    ConfiguredHttpsOrigin: "CONFIGURED HTTPS ORIGIN",
    OsAssignedLoopback: "OS-ASSIGNED LOOPBACK",
    DynamicallyDiscoveredLoopback: "DYNAMICALLY DISCOVERED LOOPBACK",
    RevalidatedLoopback: "REVALIDATED LOOPBACK",
    UnavailableFailClosed: "UNAVAILABLE FAIL-CLOSED",
  };
  const ENDPOINT_CAUTION = new Set(["UnavailableFailClosed"]);

  const PORT_LABELS = {
    NotApplicable: "NOT APPLICABLE",
    FixedConfigured: "FIXED CONFIGURED",
    OsAssigned: "OS ASSIGNED",
    DynamicallyDiscovered: "DYNAMICALLY DISCOVERED",
    ChangedAndRevalidated: "CHANGED AND REVALIDATED",
    UnavailableFailClosed: "UNAVAILABLE FAIL-CLOSED",
  };
  const PORT_CAUTION = new Set(["UnavailableFailClosed"]);

  const ROUTE_LABELS = {
    AnthropicMessages: "ANTHROPIC MESSAGES",
    Unknown: "UNKNOWN",
    Rejected: "REJECTED",
  };
  const ROUTE_CAUTION = new Set(["Rejected"]);

  const STAGE_LABELS = {
    OwnerRequest: "OWNER REQUEST",
    ProviderRequest: "PROVIDER REQUEST",
    ProviderResponse: "PROVIDER RESPONSE",
    OwnerRestoredResponse: "OWNER-RESTORED RESPONSE",
  };

  const OMISSION_LABELS = {
    NotCapturedByPolicy: "NOT CAPTURED BY POLICY",
    NotApplicable: "NOT APPLICABLE",
    UnsupportedFormat: "UNSUPPORTED FORMAT",
    MalformedFailClosed: "MALFORMED — FAIL-CLOSED",
    LimitExceeded: "LIMIT EXCEEDED",
    SignedOrEncryptedSurface: "SIGNED OR ENCRYPTED SURFACE",
    Purging: "PURGING",
    Disabled: "DISABLED",
    QueueClosed: "QUEUE CLOSED",
    ProjectionFailedClosed: "PROJECTION FAILED CLOSED",
    UnknownFuture: "UNKNOWN",
  };
  const OMISSION_NEUTRAL = new Set(["NotCapturedByPolicy", "NotApplicable"]);

  const ENGINE_LABELS = {
    DeterministicRecognizer: "DETERMINISTIC RECOGNIZER",
    Ner: "NER",
    NeuralSafetyNet: "NEURAL SAFETY NET",
    ResidualScan: "RESIDUAL SCAN",
  };

  const OUTCOME_LABELS = {
    RanClean: "RAN — NO FINDINGS (not a verified-clean claim)",
    RanFindings: "FINDINGS",
    SkippedNotApplicable: "NOT APPLICABLE",
    UnavailableFailClosed: "FAIL-CLOSED — UNAVAILABLE",
    TimeoutFailClosed: "FAIL-CLOSED — TIMEOUT",
    ErrorFailClosed: "FAIL-CLOSED — ERROR",
  };
  const OUTCOME_CAUTION = new Set([
    "RanFindings", "UnavailableFailClosed", "TimeoutFailClosed", "ErrorFailClosed",
  ]);

  const ATTESTATION_LABELS = {
    NotApplicable: "NOT APPLICABLE",
    ExactHit: "EXACT HIT",
    MissNoRecord: "MISS — NO RECORD",
    MissExpired: "MISS — EXPIRED",
    MissEvicted: "MISS — EVICTED",
    InvalidatedContentBytes: "INVALIDATED — CONTENT BYTES",
    InvalidatedStructuralIdentity: "INVALIDATED — STRUCTURAL IDENTITY",
    InvalidatedPolicyOrRulepack: "INVALIDATED — POLICY OR RULEPACK",
    InvalidatedRecognizerOrArtifact: "INVALIDATED — RECOGNIZER OR ARTIFACT",
    InvalidatedLocaleOrDictionary: "INVALIDATED — LOCALE OR DICTIONARY",
    InvalidatedNormalizationVersion: "INVALIDATED — NORMALIZATION VERSION",
    InvalidatedSessionEpoch: "INVALIDATED — SESSION EPOCH",
    InvalidatedConfigurationGeneration: "INVALIDATED — CONFIGURATION GENERATION",
    AmbiguousFailClosed: "AMBIGUOUS FAIL-CLOSED",
    UnavailableFailClosed: "UNAVAILABLE FAIL-CLOSED",
    UnknownFuture: "UNKNOWN",
  };
  const ATTESTATION_NEUTRAL = new Set(["NotApplicable", "ExactHit"]);

  const PII_CLASS_LABELS = {
    PersonName: "PERSON NAME",
    Email: "EMAIL",
    Phone: "PHONE",
    Address: "ADDRESS",
    Organization: "ORGANIZATION",
    Credential: "CREDENTIAL",
    OtherReviewed: "OTHER REVIEWED",
    UnknownFuture: "UNKNOWN",
  };

  const JSON_TYPE_LABELS = {
    Object: "OBJECT",
    Array: "ARRAY",
    String: "STRING",
    Number: "NUMBER",
    Boolean: "BOOLEAN",
    Null: "NULL",
  };

  const TRUNCATION_LABELS = {
    Complete: "COMPLETE",
    NodeLimit: "TRUNCATED — NODE LIMIT",
    DepthLimit: "TRUNCATED — DEPTH LIMIT",
    UnknownFuture: "UNKNOWN",
  };
  const TRUNCATION_NEUTRAL = new Set(["Complete"]);

  const SSE_KIND_LABELS = {
    MessageStart: "MESSAGE START",
    ContentBlockStart: "CONTENT BLOCK START",
    ContentBlockDelta: "CONTENT BLOCK DELTA",
    ContentBlockStop: "CONTENT BLOCK STOP",
    MessageDelta: "MESSAGE DELTA",
    MessageStop: "MESSAGE STOP",
    Ping: "PING",
    Error: "ERROR",
    UnknownFuture: "UNKNOWN",
  };
  const SSE_KIND_CAUTION = new Set(["Error"]);

  const SSE_DELTA_LABELS = {
    Text: "TEXT",
    InputJson: "INPUT JSON",
    Thinking: "THINKING",
    Signature: "SIGNATURE",
    Citation: "CITATION",
    UnknownFuture: "UNKNOWN",
  };

  const STATUS_LABELS = {
    Http2xx: "HTTP 2XX",
    Http4xx: "HTTP 4XX",
    Http5xx: "HTTP 5XX",
    Rejected: "REJECTED",
    Unknown: "UNKNOWN",
  };
  const STATUS_CAUTION = new Set(["Http5xx", "Rejected"]);

  const DURATION_LABELS = {
    Under1s: "UNDER 1 S",
    Under10s: "UNDER 10 S",
    Over10s: "OVER 10 S",
    Unknown: "UNKNOWN",
  };

  const DISABLED_LABELS = {
    ChildExit: "CHILD EXIT",
    IpcFault: "IPC FAULT",
    PurgeTimeout: "PURGE TIMEOUT",
    Rotation: "ROTATION",
    Shutdown: "SHUTDOWN",
    ConnectionLost: "CONNECTION LOST",
    UnknownFuture: "UNKNOWN",
  };

  const CAPTURE_BADGES = {
    ProviderVisible: "PROVIDER-VISIBLE CAPTURE — confidential pseudonymized payload, not verified clean",
    OwnerRaw: "OWNER RAW CAPTURE ENABLED",
    OwnerRestored: "OWNER RESTORED CAPTURE ENABLED",
    Both: "BOTH OWNER DOMAINS CAPTURED",
  };

  const LANES = [
    { key: "ownerRaw", cls: "lane-ownerraw", glyph: "[RAW]", label: "OWNER RAW — PII may be present" },
    { key: "providerVisible", cls: "lane-providervisible", glyph: "[PSEUDO]", label: "PROVIDER VISIBLE — pseudonymized, not verified" },
    { key: "ownerRestored", cls: "lane-ownerrestored", glyph: "[RESTORED]", label: "OWNER RESTORED — re-identified PII may be present" },
  ];

  const QUEUE_LIMITATION = "QUEUE TELEMETRY: UNAVAILABLE — NOT MEASURED";
  const DISCONNECT_BODY = "Dashboard data was purged. The proxy is unaffected and continues running without capture.";

  const TOKEN_RE = /^[A-Za-z0-9_-]{43}$/;
  const REVEAL_WINDOW_MS = 30000;
  const FOLLOW_INTERVAL_MS = 2000;
  const SSE_WINDOW = 200;
  const MAX_DEPTH_INDENT = 32;

  function label(table, code) {
    return Object.prototype.hasOwnProperty.call(table, code) ? table[code] : UNKNOWN_LABEL;
  }

  /* ================= text sanitation ================= */

  /* Bidi/control/zero-width code points render as visible placeholders in
   * LTR plain text; nothing is interpreted. */
  function san(input) {
    const s = String(input);
    let out = "";
    for (const ch of s) {
      const cp = ch.codePointAt(0);
      const isControl = cp <= 0x1f || (cp >= 0x7f && cp <= 0x9f);
      const isBidiOrZw =
        (cp >= 0x200b && cp <= 0x200f) ||
        (cp >= 0x2028 && cp <= 0x202e) ||
        (cp >= 0x2066 && cp <= 0x2069) ||
        cp === 0xfeff || cp === 0x061c;
      if (isControl || isBidiOrZw) {
        if (ch === "\n" || ch === "\t") { out += ch; continue; }
        out += "⟦U+" + cp.toString(16).toUpperCase().padStart(4, "0") + "⟧";
      } else {
        out += ch;
      }
    }
    return out;
  }

  function boundedCount(v) {
    const n = Number(v);
    if (!Number.isFinite(n) || n < 0) { return 0; }
    return Math.min(Math.floor(n), 999999999);
  }

  /* ================= DOM helpers ================= */

  function el(tag, opts, kids) {
    const node = document.createElement(tag);
    if (opts) {
      if (opts.className) { node.className = opts.className; }
      if (opts.text !== undefined && opts.text !== null) { node.textContent = san(opts.text); }
      if (opts.attrs) {
        for (const key of Object.keys(opts.attrs)) {
          node.setAttribute(key, String(opts.attrs[key]));
        }
      }
    }
    if (kids) { for (const kid of kids) { if (kid) { node.appendChild(kid); } } }
    return node;
  }

  function txt(s) { return document.createTextNode(san(s)); }

  function clear(node) { node.replaceChildren(); }

  /* ================= announcements ================= */

  const statusRegion = document.getElementById("live-status");
  let statusClearTimer = 0;

  function announce(message) {
    if (!statusRegion) { return; }
    if (statusClearTimer) { clearTimeout(statusClearTimer); statusClearTimer = 0; }
    statusRegion.textContent = "";
    // Re-set on the next frame so identical repeated messages re-announce.
    requestAnimationFrame(() => {
      statusRegion.textContent = san(message).slice(0, 300);
      statusClearTimer = setTimeout(() => { statusRegion.textContent = ""; }, 10000);
    });
  }

  /* ================= session state ================= */

  // Closure-scoped page memory only. Never persisted anywhere.
  let pageSession = null;
  let csrfToken = null;

  const state = {
    screen: "preauth",       // preauth | app
    snapshot: null,
    selectedId: null,
    singleView: "list",      // list | detail (single-column mode only)
    tab: "overview",
    lastFocusedRow: null,
    follow: {
      timer: 0,
      pausedManual: false,
      pauseReasons: new Set(),
      buffered: 0,
      pending: null,
    },
    reveal: null,            // { laneKey, logicalId, stage, emissionId, timer, region, control }
    payloadShown: null,      // provider-visible shown payload region
    terminal: null,          // null | {kind, code}
  };

  const controllers = new Set();

  function abortAll() {
    for (const c of controllers) { try { c.abort(); } catch (_e) { /* already dead */ } }
    controllers.clear();
  }

  function wipeSession() {
    pageSession = null;
    csrfToken = null;
  }

  /* ================= binary envelope decoding =================
   * Dev-fixture wire shape; the exact production envelope layout is owned by
   * the core crate and reconciled at integration (see evidence doc). */

  function b64url(bytes) {
    let bin = "";
    for (const b of bytes) { bin += String.fromCharCode(b); }
    return btoa(bin).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
  }

  function decodeBootstrap(buf) {
    const v = new Uint8Array(buf);
    if (v.length !== 70) { return null; }
    if (v[0] !== 0x47 || v[1] !== 0x5a || v[2] !== 0x44 || v[3] !== 0x42) { return null; } // GZDB
    if (v[4] !== 0x01 || v[5] !== 0x02) { return null; }
    return { page: b64url(v.subarray(6, 38)), csrf: b64url(v.subarray(38, 70)) };
  }

  function decodePayload(buf) {
    const v = new Uint8Array(buf);
    if (v.length < 11) { return null; }
    if (v[0] !== 0x47 || v[1] !== 0x5a || v[2] !== 0x50 || v[3] !== 0x4c) { return null; } // GZPL
    if (v[4] !== 0x01) { return null; }
    const domainTag = v[5];
    const stageTag = v[6];
    const len = (v[7] << 24 | v[8] << 16 | v[9] << 8 | v[10]) >>> 0;
    if (len > 4194304 || 11 + len !== v.length) { return null; }
    let text;
    try {
      text = new TextDecoder("utf-8", { fatal: true }).decode(v.subarray(11));
    } catch (_e) {
      return null;
    }
    if (domainTag < 1 || domainTag > 3 || stageTag < 1 || stageTag > 4) { return null; }
    return { text };
  }

  /* ================= network ================= */

  function baseFetch(path, init) {
    const controller = new AbortController();
    controllers.add(controller);
    const merged = Object.assign({
      method: "POST",
      credentials: "omit",
      cache: "no-store",
      redirect: "error",
      referrerPolicy: "no-referrer",
      signal: controller.signal,
    }, init);
    return fetch(path, merged).finally(() => { controllers.delete(controller); });
  }

  function authedHeaders(extra) {
    const h = Object.assign({
      "content-type": "application/json",
      "x-gaze-page-session": pageSession || "",
      "x-gaze-csrf": csrfToken || "",
    }, extra || {});
    return h;
  }

  async function authedJson(path, body) {
    const res = await baseFetch(path, {
      headers: authedHeaders(),
      body: JSON.stringify(body || {}),
    });
    if (res.status === 401 || res.status === 403) { throw { authLoss: true }; }
    if (!res.ok) { throw { httpStatus: res.status }; }
    return res.json();
  }

  async function authedPayload(path, body) {
    const res = await baseFetch(path, {
      headers: authedHeaders(),
      body: JSON.stringify(body || {}),
    });
    if (res.status === 401 || res.status === 403) { throw { authLoss: true }; }
    if (!res.ok) { throw { httpStatus: res.status }; }
    return decodePayload(await res.arrayBuffer());
  }

  /* ================= lifecycle clearing ================= */

  function concealReveal(reason, options) {
    const r = state.reveal;
    if (!r) { return; }
    if (r.timer) { clearTimeout(r.timer); }
    const hadFocusInside = r.region && r.region.contains(document.activeElement);
    if (r.region && r.region.isConnected) { r.region.remove(); }
    state.reveal = null;
    if (hadFocusInside && r.control && r.control.isConnected && (!options || !options.noRefocus)) {
      r.control.focus();
    }
    recomputeFollowPause();
    announce("Owner payload concealed — " + reason + ".");
  }

  function removeShownPayload() {
    if (state.payloadShown && state.payloadShown.isConnected) { state.payloadShown.remove(); }
    state.payloadShown = null;
  }

  function clearAllPayloadDom(reason, options) {
    concealReveal(reason, options);
    removeShownPayload();
  }

  function hardTeardown(statusMessage) {
    abortAll();
    stopFollow();
    if (state.reveal && state.reveal.timer) { clearTimeout(state.reveal.timer); }
    state.reveal = null;
    state.payloadShown = null;
    state.snapshot = null;
    state.selectedId = null;
    state.terminal = null;
    wipeSession();
    const app = document.getElementById("main");
    if (app) { clear(app); app.hidden = true; }
    const pre = document.getElementById("preauth");
    if (pre) { pre.hidden = false; }
    const input = document.getElementById("token-input");
    if (input) { input.value = ""; }
    state.screen = "preauth";
    if (statusMessage) { announce(statusMessage); }
  }

  function onAuthLoss() {
    hardTeardown("Session ended. Pair again to continue.");
  }

  /* Hidden/freeze/pagehide must leave no launch token typed, enabled, or in
   * flight — on the preauth screen as well as the paired app. Re-entry after
   * a hide requires a fresh service-worker absence proof. */
  window.addEventListener("pagehide", () => { hardTeardown(null); disablePairEntry(); });
  document.addEventListener("freeze", () => { hardTeardown(null); disablePairEntry(); });
  document.addEventListener("visibilitychange", () => {
    if (document.visibilityState === "hidden") {
      if (state.screen === "app") { hardTeardown(null); } else { abortAll(); }
      disablePairEntry();
      return;
    }
    if (document.visibilityState === "visible" && state.screen === "preauth") {
      proveSwAbsentAndEnable();
    }
  });
  window.addEventListener("pageshow", (ev) => {
    if (ev.persisted) {
      hardTeardown("Restored page discarded. Pair again to continue.");
      disablePairEntry();
      proveSwAbsentAndEnable();
    }
  });
  window.addEventListener("offline", () => {
    if (state.screen === "app") { enterDisconnected("ConnectionLost"); }
  });

  /* ================= terminal states ================= */

  function renderTerminal(root, kind, code) {
    clear(root);
    let box;
    if (kind === "disabled") {
      box = el("section", { className: "terminal caution", attrs: { "aria-labelledby": "term-title" } }, [
        el("h2", { text: "DASHBOARD DISABLED (" + label(DISABLED_LABELS, code) + ")", attrs: { id: "term-title", tabindex: "-1" } }),
        el("p", { text: DISCONNECT_BODY }),
        el("p", { className: "code", text: "Relaunch the proxy with --dashboard to start a new dashboard." }),
      ]);
    } else if (kind === "shutdown") {
      box = el("section", { className: "terminal", attrs: { "aria-labelledby": "term-title" } }, [
        el("h2", { text: "DASHBOARD SHUT DOWN — ALL DATA PURGED", attrs: { id: "term-title", tabindex: "-1" } }),
        el("p", { text: DISCONNECT_BODY }),
      ]);
    } else { // purged
      box = el("section", { className: "terminal", attrs: { "aria-labelledby": "term-title" } }, [
        el("h2", { text: "DASHBOARD PURGED", attrs: { id: "term-title", tabindex: "-1" } }),
        el("p", { text: "Every captured event, payload, and reveal was destroyed. The proxy is unaffected. New events may appear after the purge completes." }),
      ]);
    }
    root.appendChild(box);
    const h = box.querySelector("h2");
    if (h) { h.focus(); }
  }

  function enterDisconnected(code) {
    abortAll();
    stopFollow();
    clearAllPayloadDom("connection lost", { noRefocus: true });
    state.snapshot = null;
    state.selectedId = null;
    wipeSession();
    state.terminal = { kind: "disabled", code };
    const app = document.getElementById("main");
    if (app) { renderTerminal(app, "disabled", code); }
    announce("Dashboard disconnected. Data purged. The proxy continues.");
  }

  /* ================= follow loop ================= */

  function stopFollow() {
    if (state.follow.timer) { clearTimeout(state.follow.timer); state.follow.timer = 0; }
  }

  function followPaused() {
    return state.follow.pausedManual || state.follow.pauseReasons.size > 0;
  }

  function recomputeFollowPause() {
    const reasons = state.follow.pauseReasons;
    if (state.reveal) { reasons.add("reveal"); } else { reasons.delete("reveal"); }
    if (state.selectedId !== null) { reasons.add("selection"); } else { reasons.delete("selection"); }
    updateFollowControls();
  }

  function scheduleFollow() {
    stopFollow();
    if (state.screen !== "app" || state.terminal) { return; }
    state.follow.timer = setTimeout(runFollowTick, FOLLOW_INTERVAL_MS);
  }

  async function runFollowTick() {
    if (state.screen !== "app" || state.terminal) { return; }
    try {
      const snap = await authedJson("/api/v1/events/follow", {});
      ingestSnapshot(snap, true);
    } catch (err) {
      if (err && err.authLoss) { onAuthLoss(); return; }
      if (!(err && err.httpStatus)) { enterDisconnected("ConnectionLost"); return; }
    }
    scheduleFollow();
  }

  function applyPendingSnapshot() {
    if (state.follow.pending) {
      const snap = state.follow.pending;
      state.follow.pending = null;
      state.follow.buffered = 0;
      commitSnapshot(snap);
    }
  }

  function resumeFollow() {
    state.follow.pausedManual = false;
    state.follow.pauseReasons.delete("scroll");
    state.follow.pauseReasons.delete("focus");
    applyPendingSnapshot();
    updateFollowControls();
    announce("Live follow resumed.");
  }

  function ingestSnapshot(snap, fromFollow) {
    if (!snap || typeof snap !== "object") { return; }
    const lifecycle = snap.runtime && snap.runtime.lifecycle;
    if (lifecycle === "Disabled") {
      state.terminal = { kind: "disabled", code: (snap.runtime && snap.runtime.disabledCode) || "UnknownFuture" };
      stopFollow();
      clearAllPayloadDom("dashboard disabled", { noRefocus: true });
      const app = document.getElementById("main");
      if (app) { renderTerminal(app, "disabled", state.terminal.code); }
      return;
    }
    if (lifecycle === "ShutdownPurged") {
      state.terminal = { kind: "shutdown" };
      stopFollow();
      clearAllPayloadDom("dashboard shutdown", { noRefocus: true });
      const app = document.getElementById("main");
      if (app) { renderTerminal(app, "shutdown"); }
      return;
    }
    const oldSnap = state.snapshot;
    if (oldSnap && snap.runtime && oldSnap.runtime && snap.runtime.epoch !== oldSnap.runtime.epoch) {
      // Producer epoch advanced: everything selected/revealed belongs to the
      // old epoch and must go.
      clearAllPayloadDom("purge epoch advanced", { noRefocus: true });
      state.selectedId = null;
    }
    if (lifecycle === "Purged") {
      clearAllPayloadDom("purged", { noRefocus: true });
      state.selectedId = null;
      state.snapshot = snap;
      const app = document.getElementById("main");
      if (app) { renderTerminal(app, "purged"); }
      return;
    }
    if (fromFollow && followPaused()) {
      const oldIds = new Set((oldSnap && oldSnap.events || []).map((e) => e.logicalId));
      const fresh = (snap.events || []).filter((e) => !oldIds.has(e.logicalId)).length;
      state.follow.pending = snap;
      if (fresh > 0) {
        state.follow.buffered += fresh;
        updateFollowControls();
      }
      return;
    }
    commitSnapshot(snap);
  }

  function commitSnapshot(snap) {
    const prevSelected = state.selectedId;
    state.snapshot = snap;
    if (prevSelected !== null) {
      const still = (snap.events || []).some((e) => e.logicalId === prevSelected);
      if (!still) {
        // The selected event ceased display: same rule as back/navigation.
        clearAllPayloadDom("event no longer displayed", { noRefocus: true });
        state.selectedId = null;
        state.singleView = "list";
      }
    }
    renderApp();
  }

  /* ================= rendering: safety bar ================= */

  function captureTierBadges(tier) {
    const badges = [CAPTURE_BADGES.ProviderVisible];
    if (tier === "OwnerRaw") { badges.push(CAPTURE_BADGES.OwnerRaw); }
    if (tier === "OwnerRestored") { badges.push(CAPTURE_BADGES.OwnerRestored); }
    if (tier === "Both") { badges.push(CAPTURE_BADGES.Both); }
    return badges;
  }

  function dropTotal(counters) {
    if (!counters) { return 0; }
    return boundedCount(counters.ingressFull) + boundedCount(counters.ingressClosed) +
      boundedCount(counters.oversize) + boundedCount(counters.capacityEvicted) +
      boundedCount(counters.staleEpoch) + boundedCount(counters.duplicateConflict);
  }

  function renderSafetyBar(snap) {
    const bar = el("div", { className: "safety-bar", attrs: { role: "group", "aria-label": "Safety state" } });
    for (const badgeText of captureTierBadges(snap.runtime.captureTier)) {
      const isOwner = badgeText !== CAPTURE_BADGES.ProviderVisible;
      bar.appendChild(el("span", {
        className: "badge capture-badge" + (isOwner ? " badge-caution" : ""),
        text: badgeText,
      }));
    }
    const drops = dropTotal(snap.counters);
    if (drops > 0) {
      const c = snap.counters || {};
      bar.appendChild(el("span", {
        className: "badge badge-caution capture-badge",
        text: "VIEW INCOMPLETE — DROPPED " + drops +
          " (FULL " + boundedCount(c.ingressFull) +
          " · CLOSED " + boundedCount(c.ingressClosed) +
          " · OVERSIZE " + boundedCount(c.oversize) +
          " · EVICTED " + boundedCount(c.capacityEvicted) +
          " · STALE EPOCH " + boundedCount(c.staleEpoch) +
          " · CONFLICT " + boundedCount(c.duplicateConflict) + ")",
      }));
    }
    bar.appendChild(el("span", {
      className: "bar-summary",
      text: "RING " + boundedCount(snap.runtime.retained) + "/" + boundedCount(snap.runtime.capacity) +
        " · TTL " + boundedCount(snap.runtime.ttlSeconds) + " S · CONNECTED · EPOCH " +
        boundedCount(snap.runtime.epoch),
    }));
    bar.appendChild(el("span", { className: "bar-spacer" }));
    const purgeBtn = el("button", { className: "btn-caution", text: "Purge now", attrs: { type: "button" } });
    purgeBtn.addEventListener("click", onPurgeClick);
    bar.appendChild(purgeBtn);
    return bar;
  }

  async function onPurgeClick() {
    try {
      await authedJson("/api/v1/purge", {});
      announce("Purge requested. All captured data is being destroyed.");
    } catch (err) {
      if (err && err.authLoss) { onAuthLoss(); return; }
      enterDisconnected("ConnectionLost");
    }
  }

  /* ================= rendering: event list (P1) ================= */

  function availabilityText(avail) {
    if (!avail || typeof avail !== "object") { return { text: "OMITTED (" + UNKNOWN_LABEL + ")", caution: true }; }
    if (avail.state === "Present") { return null; }
    const reason = avail.reason;
    return {
      text: "OMITTED (" + label(OMISSION_LABELS, reason) + ")",
      caution: !OMISSION_NEUTRAL.has(reason),
    };
  }

  function eventCautionMarks(ev) {
    const marks = [];
    if (ROUTE_CAUTION.has(ev.route)) { marks.push(label(ROUTE_LABELS, ev.route)); }
    if (STATUS_CAUTION.has(ev.statusClass)) { marks.push(label(STATUS_LABELS, ev.statusClass)); }
    if (ENDPOINT_CAUTION.has(ev.endpoint)) { marks.push(label(ENDPOINT_LABELS, ev.endpoint)); }
    if (PORT_CAUTION.has(ev.port)) { marks.push(label(PORT_LABELS, ev.port)); }
    return marks;
  }

  function piiSummaryText(ev) {
    const omission = availabilityText(ev.piiSummary);
    if (omission) { return "PII " + omission.text; }
    const entries = ev.piiSummary.value || [];
    if (entries.length === 0) { return "PII: NONE REPORTED (not a verified-clean claim)"; }
    return "PII " + entries.map((p) =>
      label(PII_CLASS_LABELS, p.cls) + " " + boundedCount(p.detected) + "/" + boundedCount(p.pseudonymized)
    ).join(" · ");
  }

  function renderEventRow(ev, position, total) {
    const selected = state.selectedId === ev.logicalId;
    const marks = eventCautionMarks(ev);
    const row = el("button", {
      className: "event-row",
      attrs: {
        type: "button",
        "data-logical-id": String(boundedCount(ev.logicalId)),
        "aria-current": selected ? "true" : "false",
      },
    }, [
      el("span", { className: "row-head" }, [
        el("span", { text: "LOG #" + boundedCount(ev.logicalId) }),
        el("span", { text: label(ROUTE_LABELS, ev.route) }),
        el("span", { text: label(STATUS_LABELS, ev.statusClass) }),
        marks.length ? el("span", { className: "row-caution", text: "⚠ " + marks.join(" · ") }) : null,
      ]),
      el("span", { className: "row-meta" }, [
        el("span", { text: label(PROFILE_LABELS, ev.profile) }),
        el("span", { text: label(ENDPOINT_LABELS, ev.endpoint) }),
        el("span", { text: "PORT: " + label(PORT_LABELS, ev.port) + (ev.portChangeCount !== undefined && ev.portChangeCount !== null ? " · SELECTION CHANGE " + boundedCount(ev.portChangeCount) : "") }),
        el("span", { text: label(DURATION_LABELS, ev.durationBucket) + " · RETRY " + boundedCount(ev.retryCount) }),
        el("span", { text: piiSummaryText(ev) }),
        el("span", { text: "TTL " + boundedCount(ev.ttlSeconds) + " S" }),
      ]),
    ]);
    row.addEventListener("click", () => { selectEvent(ev.logicalId, row); });
    return row;
  }

  function renderList(snap) {
    const pane = el("section", { className: "pane pane-list", attrs: { "aria-labelledby": "list-title" } });
    pane.appendChild(el("h2", { className: "pane-title", text: "EVENTS — NEWEST FIRST", attrs: { id: "list-title" } }));

    const controlsRow = el("div", { className: "follow-controls" });
    const followState = el("span", { className: "follow-state", attrs: { id: "follow-state" } });
    const resumeBtn = el("button", { attrs: { type: "button", id: "resume-follow" }, text: "Resume" });
    resumeBtn.addEventListener("click", resumeFollow);
    controlsRow.appendChild(followState);
    controlsRow.appendChild(resumeBtn);
    pane.appendChild(controlsRow);
    pane.appendChild(el("p", { className: "limitation", text: QUEUE_LIMITATION }));

    const events = (snap.events || []).slice();
    if (events.length === 0) {
      pane.appendChild(el("p", { className: "lane-state", text: "NO EVENTS CAPTURED YET." }));
      pane.appendChild(el("p", { className: "limitation", text: "An empty list is not a claim that no traffic exists — only that nothing was captured and retained." }));
      return pane;
    }
    const list = el("ul", { className: "event-list", attrs: { id: "event-list" } });
    events.forEach((ev, i) => {
      list.appendChild(el("li", { attrs: { "aria-setsize": String(events.length), "aria-posinset": String(i + 1) } }, [renderEventRow(ev, i + 1, events.length)]));
    });
    list.addEventListener("focusin", () => {
      state.follow.pauseReasons.add("focus");
      updateFollowControls();
    });
    list.addEventListener("keydown", (kev) => {
      if (kev.key !== "ArrowDown" && kev.key !== "ArrowUp") { return; }
      state.follow.pauseReasons.add("focus");
      const rows = Array.from(list.querySelectorAll(".event-row"));
      const idx = rows.indexOf(document.activeElement);
      if (idx === -1) { return; }
      kev.preventDefault();
      const next = kev.key === "ArrowDown" ? Math.min(idx + 1, rows.length - 1) : Math.max(idx - 1, 0);
      rows[next].focus();
    });
    pane.appendChild(list);
    return pane;
  }

  function updateFollowControls() {
    const stateEl = document.getElementById("follow-state");
    const resume = document.getElementById("resume-follow");
    if (!stateEl || !resume) { return; }
    if (followPaused()) {
      stateEl.textContent = san("FOLLOW PAUSED — " + boundedCount(state.follow.buffered) + " BUFFERED");
      resume.hidden = false;
      resume.setAttribute("aria-label",
        "Resume live follow (" + boundedCount(state.follow.buffered) + " buffered events)");
    } else {
      stateEl.textContent = san("FOLLOW: LIVE");
      resume.hidden = true;
    }
  }

  /* ================= selection / navigation ================= */

  function layoutMode() {
    if (window.matchMedia("(min-width: 1024px)").matches) { return "side"; }
    if (window.matchMedia("(min-width: 768px)").matches) { return "stacked"; }
    return "single";
  }

  function selectEvent(logicalId, rowNode) {
    // Selection change conceals/removes any payload before anything else.
    clearAllPayloadDom("selection changed", { noRefocus: true });
    state.selectedId = logicalId;
    state.tab = "overview";
    state.lastFocusedRow = rowNode ? rowNode.getAttribute("data-logical-id") : null;
    if (layoutMode() === "single") { state.singleView = "detail"; }
    recomputeFollowPause();
    renderApp();
    const heading = document.getElementById("detail-title");
    if (heading) { heading.focus(); }
  }

  function backToList() {
    clearAllPayloadDom("navigated back", { noRefocus: true });
    const restoreId = state.lastFocusedRow;
    state.selectedId = null;
    state.singleView = "list";
    recomputeFollowPause();
    renderApp();
    if (restoreId !== null) {
      const row = document.querySelector('.event-row[data-logical-id="' + restoreId + '"]');
      if (row) { row.focus(); }
    }
  }

  /* ================= rendering: detail (P2) ================= */

  function kvRow(dl, term, value, caution) {
    dl.appendChild(el("dt", { text: term }));
    dl.appendChild(el("dd", { text: value, className: caution ? "caution" : "" }));
  }

  function renderOverviewTab(ev) {
    const wrap = el("div", { className: "tabpanel-body" });
    const dl = el("dl", { className: "kv" });
    kvRow(dl, "LOGICAL ID", "LOG #" + boundedCount(ev.logicalId));
    kvRow(dl, "ROUTE", label(ROUTE_LABELS, ev.route), ROUTE_CAUTION.has(ev.route));
    kvRow(dl, "PROVIDER PROFILE", label(PROFILE_LABELS, ev.profile));
    kvRow(dl, "ENDPOINT", label(ENDPOINT_LABELS, ev.endpoint), ENDPOINT_CAUTION.has(ev.endpoint));
    let portText = label(PORT_LABELS, ev.port);
    if (ev.portChangeCount !== undefined && ev.portChangeCount !== null) {
      portText += " · SELECTION CHANGE " + boundedCount(ev.portChangeCount);
    }
    kvRow(dl, "PORT", portText, PORT_CAUTION.has(ev.port));
    kvRow(dl, "STATUS CLASS", label(STATUS_LABELS, ev.statusClass), STATUS_CAUTION.has(ev.statusClass));
    kvRow(dl, "DURATION", label(DURATION_LABELS, ev.durationBucket));
    kvRow(dl, "RETRIES", String(boundedCount(ev.retryCount)));
    kvRow(dl, "PRODUCER EPOCH", String(boundedCount(ev.epoch)));
    kvRow(dl, "TTL REMAINING", boundedCount(ev.ttlSeconds) + " S");
    const stages = ev.stages || {};
    for (const stageKey of ["ownerRequest", "providerRequest", "providerResponse", "ownerRestoredResponse"]) {
      const stage = stages[stageKey];
      const stageLabel = label(STAGE_LABELS, stageKey.charAt(0).toUpperCase() + stageKey.slice(1));
      if (!stage) { kvRow(dl, stageLabel, "NO EMISSION"); continue; }
      const m = availabilityText(stage.measurements);
      const measurement = m ? m.text :
        boundedCount(stage.measurements.value.bytes) + " B · " +
        boundedCount(stage.measurements.value.chunks) + " CHUNKS";
      kvRow(dl, stageLabel, "EMISSION #" + boundedCount(stage.emissionId) + " · " + measurement, Boolean(m && m.caution));
    }
    const pii = availabilityText(ev.piiSummary);
    if (pii) {
      kvRow(dl, "PII SUMMARY", pii.text, pii.caution);
    } else {
      const entries = ev.piiSummary.value || [];
      kvRow(dl, "PII SUMMARY", entries.length === 0 ?
        "NONE REPORTED (not a verified-clean claim)" :
        entries.map((p) => label(PII_CLASS_LABELS, p.cls) + " detected " +
          boundedCount(p.detected) + ", pseudonymized " + boundedCount(p.pseudonymized)).join("; "));
    }
    wrap.appendChild(dl);
    wrap.appendChild(el("p", { className: "limitation", text: QUEUE_LIMITATION }));
    return wrap;
  }

  function laneStateNode(kind, reasonCode) {
    if (kind === "NotCaptured") {
      return el("p", { className: "lane-state", text: "NOT CAPTURED — this domain was not authorized at startup. No reveal is possible." });
    }
    if (kind === "Concealed") {
      return el("p", { className: "lane-state", text: "CONCEALED — captured and retained. Reveal below displays it for 30 seconds." });
    }
    if (kind === "Available") {
      return el("p", { className: "lane-state", text: "AVAILABLE" });
    }
    if (kind === "Omitted") {
      return el("p", {
        className: "lane-state" + (OMISSION_NEUTRAL.has(reasonCode) ? "" : " caution"),
        text: "PAYLOAD OMITTED (" + label(OMISSION_LABELS, reasonCode) + ")",
      });
    }
    if (kind === "Expired") { return el("p", { className: "lane-state caution", text: "EXPIRED — record TTL passed. No data retained." }); }
    if (kind === "Purged") { return el("p", { className: "lane-state caution", text: "PURGED — record belonged to an older epoch. No data retained." }); }
    return el("p", { className: "lane-state caution", text: "UNKNOWN STATE — FAIL-CLOSED" });
  }

  function revealMetaLine(laneLabel, ev, stageName, emissionId) {
    return el("p", {
      className: "reveal-meta",
      text: laneLabel + " · LOG #" + boundedCount(ev.logicalId) + " · " +
        label(STAGE_LABELS, stageName) + " · EMISSION #" + boundedCount(emissionId),
    });
  }

  function renderOwnerLane(lane, ev, stageKeyName, domainField, revealPath) {
    const stages = ev.stages || {};
    const stage = stages[stageKeyName];
    const box = el("section", { className: "lane " + lane.cls, attrs: { "aria-label": lane.label } });
    box.appendChild(el("h4", { className: "lane-label" }, [
      el("span", { className: "lane-glyph", text: lane.glyph, attrs: { "aria-hidden": "true" } }),
      txt(lane.label),
    ]));
    const d = stage && stage.domainState ? stage.domainState : { kind: "NotCaptured" };
    box.appendChild(laneStateNode(d.kind, d.reason));
    if (d.kind === "Concealed" && stage) {
      const btn = el("button", {
        attrs: { type: "button" },
        text: "Reveal " + domainField + " (30 s window)",
      });
      btn.addEventListener("click", () => {
        openReveal(lane, ev, stageKeyName, stage, revealPath, box, btn);
      });
      box.appendChild(btn);
    }
    return box;
  }

  function stageName(stageKey) {
    return stageKey.charAt(0).toUpperCase() + stageKey.slice(1);
  }

  function renderProviderLane(lane, ev) {
    const stages = ev.stages || {};
    const box = el("section", { className: "lane " + lane.cls, attrs: { "aria-label": lane.label } });
    box.appendChild(el("h4", { className: "lane-label" }, [
      el("span", { className: "lane-glyph", text: lane.glyph, attrs: { "aria-hidden": "true" } }),
      txt(lane.label),
    ]));
    for (const stageKey of ["providerRequest", "providerResponse"]) {
      const stage = stages[stageKey];
      const sub = el("div", { className: "substage" });
      sub.appendChild(el("h5", { className: "substage-title", text: label(STAGE_LABELS, stageName(stageKey)) }));
      if (!stage) {
        sub.appendChild(el("p", { className: "lane-state", text: "NO EMISSION" }));
        box.appendChild(sub);
        continue;
      }
      // ProviderVisible has no NOT CAPTURED branch by contract; unknown kinds
      // fall closed to the caution UNKNOWN state.
      const d = stage.domainState || {};
      const kind = d.kind === "NotCaptured" ? "unknown-fail-closed" : d.kind;
      sub.appendChild(laneStateNode(kind, d.reason));
      if (kind === "Available") {
        const btn = el("button", { attrs: { type: "button" }, text: "Show pseudonymized payload" });
        btn.addEventListener("click", () => {
          showProviderPayload(lane, ev, stageKey, stage, sub, btn);
        });
        sub.appendChild(btn);
      }
      box.appendChild(sub);
    }
    return box;
  }

  function renderDomainsTab(ev) {
    const wrap = el("div", { className: "lanes" });
    wrap.appendChild(renderOwnerLane(LANES[0], ev, "ownerRequest", "owner raw", "/api/v1/reveal/owner-raw"));
    wrap.appendChild(renderProviderLane(LANES[1], ev));
    wrap.appendChild(renderOwnerLane(LANES[2], ev, "ownerRestoredResponse", "owner restored", "/api/v1/reveal/owner-restored"));
    return wrap;
  }

  async function showProviderPayload(lane, ev, stageKey, stage, container, control) {
    removeShownPayload();
    let decoded = null;
    try {
      decoded = await authedPayload("/api/v1/events/provider-visible", {
        logicalId: ev.logicalId,
        stage: stageName(stageKey),
        emissionId: stage.emissionId,
      });
    } catch (err) {
      if (err && err.authLoss) { onAuthLoss(); return; }
      announce("Payload request failed closed. Nothing was displayed.");
      return;
    }
    if (!decoded) {
      announce("Payload response was malformed. Nothing was displayed.");
      return;
    }
    const region = el("div", { className: "payload-region" }, [
      revealMetaLine(lane.label, ev, stageName(stageKey), stage.emissionId),
      el("pre", { className: "payload", text: decoded.text }),
      (() => {
        const b = el("button", { attrs: { type: "button" }, text: "Remove payload from page" });
        b.addEventListener("click", () => {
          removeShownPayload();
          control.focus();
          announce("Provider-visible payload removed.");
        });
        return b;
      })(),
    ]);
    container.appendChild(region);
    state.payloadShown = region;
    announce("Provider-visible pseudonymized payload displayed. It is not verified clean.");
  }

  async function openReveal(lane, ev, stageKey, stage, path, container, control) {
    concealReveal("replaced by a new reveal", { noRefocus: true });
    let decoded = null;
    try {
      decoded = await authedPayload(path, {
        logicalId: ev.logicalId,
        stage: stageName(stageKey),
        emissionId: stage.emissionId,
      });
    } catch (err) {
      if (err && err.authLoss) { onAuthLoss(); return; }
      announce("Reveal request failed closed. Nothing was revealed.");
      return;
    }
    if (!decoded) {
      announce("Reveal response was malformed. Nothing was revealed.");
      return;
    }
    const concealBtn = el("button", { attrs: { type: "button" }, text: "Conceal now" });
    const region = el("div", { className: "payload-region" }, [
      revealMetaLine(lane.label, ev, stageName(stageKey), stage.emissionId),
      el("p", { className: "reveal-meta", text: "WINDOW: 30 S — conceals automatically. Re-revealing starts a fresh window, never an extension." }),
      el("pre", { className: "payload", text: decoded.text }),
      concealBtn,
    ]);
    container.appendChild(region);
    const reveal = {
      laneKey: lane.key,
      logicalId: ev.logicalId,
      stage: stageKey,
      emissionId: stage.emissionId,
      region,
      control,
      timer: 0,
    };
    reveal.timer = setTimeout(() => { concealReveal("30-second window expired"); }, REVEAL_WINDOW_MS);
    state.reveal = reveal;
    concealBtn.addEventListener("click", () => { concealReveal("concealed manually"); });
    recomputeFollowPause();
    announce(lane.label + " payload revealed for logical " + boundedCount(ev.logicalId) +
      ". It conceals automatically in 30 seconds.");
  }

  function renderStructureTab(ev) {
    const wrap = el("div", { className: "tabpanel-body" });
    const omission = availabilityText(ev.structure);
    if (omission) {
      wrap.appendChild(el("p", { className: OMISSION_NEUTRAL.has(ev.structure && ev.structure.reason) ? "limitation" : "omission", text: "STRUCTURE " + omission.text }));
      return wrap;
    }
    const shape = ev.structure.value || {};
    const nodes = shape.nodes || [];
    if (!TRUNCATION_NEUTRAL.has(shape.truncation)) {
      wrap.appendChild(el("p", { className: "omission", text: label(TRUNCATION_LABELS, shape.truncation) }));
    }
    const tableWrap = el("div", { className: "table-wrap" });
    const table = el("table", { className: "data-table" });
    table.appendChild(el("caption", { text: "JSON SKELETON — ordinals and types only; keys and values are never shown here." }));
    const thead = el("thead", null, [el("tr", null, [
      el("th", { text: "ORDINAL", attrs: { scope: "col" } }),
      el("th", { text: "TYPE", attrs: { scope: "col" } }),
      el("th", { text: "DEPTH", attrs: { scope: "col" } }),
      el("th", { text: "CHILDREN", attrs: { scope: "col" } }),
    ])]);
    table.appendChild(thead);
    const tbody = el("tbody");
    for (const node of nodes.slice(0, 1024)) {
      const depth = Math.min(boundedCount(node.depth), 64);
      const tr = el("tr", { className: "json-row" });
      const ordCell = el("td", { text: "#" + boundedCount(node.ordinal) });
      // Finite clamped numeric indent; depth is capped twice (data + render).
      ordCell.style.paddingLeft = (4 + Math.min(depth, MAX_DEPTH_INDENT) * 8) + "px";
      tr.appendChild(ordCell);
      tr.appendChild(el("td", { text: label(JSON_TYPE_LABELS, node.type) + " · D" + depth }));
      tr.appendChild(el("td", { text: String(depth) }));
      tr.appendChild(el("td", { text: node.children === undefined ? "—" : String(boundedCount(node.children)) }));
      tbody.appendChild(tr);
    }
    table.appendChild(tbody);
    tableWrap.appendChild(table);
    wrap.appendChild(tableWrap);
    return wrap;
  }

  const streamWindow = { start: 0 };

  function renderStreamTab(ev) {
    const wrap = el("div", { className: "tabpanel-body" });
    const omission = availabilityText(ev.stream);
    if (omission) {
      wrap.appendChild(el("p", { className: OMISSION_NEUTRAL.has(ev.stream && ev.stream.reason) ? "limitation" : "omission", text: "STREAM " + omission.text }));
      return wrap;
    }
    const entries = (ev.stream.value && ev.stream.value.entries) || [];
    const total = entries.length;
    wrap.appendChild(el("p", { className: "limitation", text: "SSE entries carry ordinal, kind, delta kind, and content-block index only. Per-entry byte counts and timing are not measured and never shown." }));
    if (total === 0) {
      wrap.appendChild(el("p", { className: "lane-state", text: "NO STREAM ENTRIES." }));
      return wrap;
    }
    if (streamWindow.start >= total) { streamWindow.start = 0; }
    const start = streamWindow.start;
    const end = Math.min(start + SSE_WINDOW, total);

    const windowing = el("div", { className: "windowing" });
    const prev = el("button", { attrs: { type: "button" }, text: "Previous " + SSE_WINDOW });
    const next = el("button", { attrs: { type: "button" }, text: "Next " + SSE_WINDOW });
    prev.disabled = start === 0;
    next.disabled = end >= total;
    prev.addEventListener("click", () => { streamWindow.start = Math.max(0, start - SSE_WINDOW); renderApp(); });
    next.addEventListener("click", () => { streamWindow.start = end; renderApp(); });
    windowing.appendChild(el("span", { text: "ENTRIES " + (start + 1) + "–" + end + " OF " + total, attrs: { role: "status" } }));
    windowing.appendChild(prev);
    windowing.appendChild(next);
    wrap.appendChild(windowing);

    const tableWrap = el("div", { className: "table-wrap" });
    const table = el("table", { className: "data-table" });
    table.appendChild(el("caption", { text: "SSE TIMELINE — entries " + (start + 1) + "–" + end + " of " + total }));
    table.appendChild(el("thead", null, [el("tr", null, [
      el("th", { text: "ORDINAL", attrs: { scope: "col" } }),
      el("th", { text: "EVENT KIND", attrs: { scope: "col" } }),
      el("th", { text: "DELTA", attrs: { scope: "col" } }),
      el("th", { text: "CONTENT BLOCK", attrs: { scope: "col" } }),
    ])]));
    const tbody = el("tbody");
    for (let i = start; i < end; i++) {
      const entry = entries[i];
      const tr = el("tr", {
        className: "sse-row" + (SSE_KIND_CAUTION.has(entry.kind) ? " trace-row-caution" : ""),
        attrs: { "aria-setsize": String(total), "aria-posinset": String(i + 1) },
      });
      tr.appendChild(el("td", { text: "#" + boundedCount(entry.ordinal) }));
      tr.appendChild(el("td", { text: label(SSE_KIND_LABELS, entry.kind) }));
      tr.appendChild(el("td", { text: entry.delta === undefined || entry.delta === null ? "—" : label(SSE_DELTA_LABELS, entry.delta) }));
      tr.appendChild(el("td", { text: entry.block === undefined || entry.block === null ? "—" : "BLOCK " + boundedCount(entry.block) }));
      tbody.appendChild(tr);
    }
    table.appendChild(tbody);
    tableWrap.appendChild(table);
    wrap.appendChild(tableWrap);
    return wrap;
  }

  function renderTraceTab(ev) {
    const wrap = el("div", { className: "tabpanel-body" });
    const omission = availabilityText(ev.decisions);
    if (omission) {
      wrap.appendChild(el("p", { className: OMISSION_NEUTRAL.has(ev.decisions && ev.decisions.reason) ? "limitation" : "omission", text: "DECISION TRACE " + omission.text }));
    } else {
      const tableWrap = el("div", { className: "table-wrap" });
      const table = el("table", { className: "data-table" });
      table.appendChild(el("caption", { text: "DECISION TRACE — engines, outcomes, and bounded counts only." }));
      table.appendChild(el("thead", null, [el("tr", null, [
        el("th", { text: "ORDINAL", attrs: { scope: "col" } }),
        el("th", { text: "ENGINE", attrs: { scope: "col" } }),
        el("th", { text: "OUTCOME", attrs: { scope: "col" } }),
        el("th", { text: "PII CLASS", attrs: { scope: "col" } }),
        el("th", { text: "DETECTED / PSEUDONYMIZED", attrs: { scope: "col" } }),
      ])]));
      const tbody = el("tbody");
      for (const d of (ev.decisions.value || []).slice(0, 256)) {
        const tr = el("tr", { className: OUTCOME_CAUTION.has(d.outcome) ? "trace-row-caution" : "" });
        tr.appendChild(el("td", { text: "#" + boundedCount(d.ordinal) }));
        tr.appendChild(el("td", { text: label(ENGINE_LABELS, d.engine) }));
        tr.appendChild(el("td", { text: label(OUTCOME_LABELS, d.outcome) }));
        tr.appendChild(el("td", { text: d.piiClass ? label(PII_CLASS_LABELS, d.piiClass) : "—" }));
        tr.appendChild(el("td", {
          text: d.detected === undefined ? "—" :
            boundedCount(d.detected) + " / " + boundedCount(d.pseudonymized),
        }));
        tbody.appendChild(tr);
      }
      table.appendChild(tbody);
      tableWrap.appendChild(table);
      wrap.appendChild(tableWrap);
    }
    const att = availabilityText(ev.attestation);
    const dl = el("dl", { className: "kv" });
    if (att) {
      kvRow(dl, "ATTESTATION", att.text, att.caution);
    } else {
      const code = ev.attestation.value;
      kvRow(dl, "ATTESTATION", label(ATTESTATION_LABELS, code), !ATTESTATION_NEUTRAL.has(code));
    }
    wrap.appendChild(dl);
    wrap.appendChild(el("p", { className: "limitation", text: "Attestation is informational. EXACT HIT is not a verification or success claim." }));
    return wrap;
  }

  const TABS = [
    { id: "overview", label: "OVERVIEW", render: renderOverviewTab },
    { id: "domains", label: "DOMAINS", render: renderDomainsTab },
    { id: "structure", label: "STRUCTURE", render: renderStructureTab },
    { id: "stream", label: "STREAM", render: renderStreamTab },
    { id: "trace", label: "TRACE", render: renderTraceTab },
  ];

  function renderDetail(snap) {
    const pane = el("section", { className: "pane pane-detail", attrs: { "aria-labelledby": "detail-title" } });
    const ev = (snap.events || []).find((e) => e.logicalId === state.selectedId);
    if (!ev) {
      pane.appendChild(el("h2", { className: "pane-title", text: "EVENT DETAIL", attrs: { id: "detail-title", tabindex: "-1" } }));
      pane.appendChild(el("p", { className: "lane-state", text: "SELECT AN EVENT FROM THE LIST." }));
      return pane;
    }
    if (layoutMode() === "single") {
      const back = el("button", { className: "back-button", attrs: { type: "button" }, text: "← Back to event list" });
      back.addEventListener("click", backToList);
      pane.appendChild(back);
    }
    pane.appendChild(el("div", { className: "detail-head" }, [
      el("h2", {
        className: "pane-title detail-title",
        text: "LOG #" + boundedCount(ev.logicalId) + " — " + label(ROUTE_LABELS, ev.route),
        attrs: { id: "detail-title", tabindex: "-1" },
      }),
    ]));

    const tablist = el("div", { className: "tablist", attrs: { role: "tablist", "aria-label": "Event detail sections" } });
    const panelHolder = el("div", { className: "tabpanel", attrs: { id: "tabpanel", role: "tabpanel" } });
    TABS.forEach((tab) => {
      const selected = state.tab === tab.id;
      const btn = el("button", {
        className: "tab",
        text: tab.label,
        attrs: {
          type: "button",
          role: "tab",
          id: "tab-" + tab.id,
          "aria-selected": selected ? "true" : "false",
          "aria-controls": "tabpanel",
          tabindex: selected ? "0" : "-1",
        },
      });
      btn.addEventListener("click", () => {
        clearAllPayloadDom("section changed", { noRefocus: true });
        state.tab = tab.id;
        renderApp();
        const focusTab = document.getElementById("tab-" + tab.id);
        if (focusTab) { focusTab.focus(); }
      });
      btn.addEventListener("keydown", (kev) => {
        if (kev.key !== "ArrowRight" && kev.key !== "ArrowLeft") { return; }
        kev.preventDefault();
        const idx = TABS.findIndex((t) => t.id === state.tab);
        const nextIdx = kev.key === "ArrowRight" ? (idx + 1) % TABS.length : (idx + TABS.length - 1) % TABS.length;
        clearAllPayloadDom("section changed", { noRefocus: true });
        state.tab = TABS[nextIdx].id;
        renderApp();
        const focusTab = document.getElementById("tab-" + TABS[nextIdx].id);
        if (focusTab) { focusTab.focus(); }
      });
      tablist.appendChild(btn);
    });
    panelHolder.setAttribute("aria-labelledby", "tab-" + state.tab);
    const active = TABS.find((t) => t.id === state.tab) || TABS[0];
    panelHolder.appendChild(active.render(ev));
    pane.appendChild(tablist);
    pane.appendChild(panelHolder);
    return pane;
  }

  /* ================= rendering: app root ================= */

  function syncBarHeight() {
    // A non-sticky bar (short viewports) scrolls away and needs no focus
    // scroll offset; a large offset would exceed the viewport and break
    // focus scrolling entirely.
    const bar = document.querySelector(".safety-bar");
    let h = 0;
    if (bar && getComputedStyle(bar).position === "sticky") {
      h = Math.max(0, Math.min(bar.offsetHeight, 400));
    }
    document.documentElement.style.setProperty("--bar-h", h + "px");
  }

  function renderApp() {
    const root = document.getElementById("main");
    if (!root || state.screen !== "app") { return; }
    if (state.terminal) {
      renderTerminal(root, state.terminal.kind, state.terminal.code);
      return;
    }
    const snap = state.snapshot;
    if (!snap) { clear(root); return; }
    clear(root);
    root.appendChild(renderSafetyBar(snap));

    const mode = layoutMode();
    const panes = el("div", { className: "panes" });
    if (mode === "single") {
      if (state.singleView === "detail" && state.selectedId !== null) {
        panes.appendChild(renderDetail(snap));
      } else {
        panes.appendChild(renderList(snap));
      }
    } else {
      panes.appendChild(renderList(snap));
      panes.appendChild(renderDetail(snap));
    }
    root.appendChild(panes);
    updateFollowControls();
    attachScrollPause();
    syncBarHeight();
  }

  /* The page/document is the real scroll container at every viewport
   * (.pane-list declares no overflow), so upward-scroll detection binds once
   * to window. The per-render element listener below stays for any future
   * element-level scroller; scroll events do not bubble, so the two paths
   * never double-fire for the same scroll. */
  let pageScrollLastTop = null;

  function onPageScrollPause() {
    const doc = document.scrollingElement || document.documentElement;
    const top = doc ? doc.scrollTop : 0;
    const last = pageScrollLastTop;
    pageScrollLastTop = top;
    if (state.screen !== "app" || state.terminal) { return; }
    if (last !== null && top < last) {
      state.follow.pauseReasons.add("scroll");
      updateFollowControls();
    }
  }

  window.addEventListener("scroll", onPageScrollPause, { passive: true });

  function attachScrollPause() {
    const list = document.getElementById("event-list");
    if (!list) { return; }
    const scroller = list.closest(".pane-list") || list;
    let lastTop = scroller.scrollTop;
    scroller.addEventListener("scroll", () => {
      const top = scroller.scrollTop;
      if (top < lastTop) {
        state.follow.pauseReasons.add("scroll");
        updateFollowControls();
      }
      lastTop = top;
    }, { passive: true });
  }

  let relayoutQueued = false;
  window.addEventListener("resize", () => {
    if (relayoutQueued) { return; }
    relayoutQueued = true;
    requestAnimationFrame(() => {
      relayoutQueued = false;
      if (state.screen === "app" && !state.terminal) { renderApp(); }
      syncBarHeight();
    });
  });

  /* ================= pairing ================= */

  const tokenInput = document.getElementById("token-input");
  const pairButton = document.getElementById("pair-button");
  const preauthAlert = document.getElementById("preauth-alert");

  function setPairError(message) {
    if (preauthAlert) { preauthAlert.textContent = san(message); }
  }

  async function attemptPair() {
    if (!tokenInput) { return; }
    const raw = tokenInput.value;
    tokenInput.value = "";
    setPairError("");
    if (!TOKEN_RE.test(raw)) {
      setPairError("Pairing failed.");
      return;
    }
    let res;
    try {
      res = await baseFetch("/api/v1/session/pair", {
        headers: {
          authorization: "GazeDashboardV1 " + raw,
          "content-type": "application/gaze-dashboard-pair-v1",
        },
        body: "GZDB-PAIR-V1",
      });
    } catch (_err) {
      setPairError("Pairing failed.");
      return;
    }
    if (!res.ok) {
      setPairError("Pairing failed.");
      return;
    }
    const secrets = decodeBootstrap(await res.arrayBuffer());
    if (!secrets) {
      setPairError("Pairing failed.");
      return;
    }
    pageSession = secrets.page;
    csrfToken = secrets.csrf;
    enterApp();
  }

  async function enterApp() {
    state.screen = "app";
    state.terminal = null;
    state.selectedId = null;
    state.singleView = "list";
    state.tab = "overview";
    state.follow.pausedManual = false;
    state.follow.pauseReasons.clear();
    state.follow.buffered = 0;
    state.follow.pending = null;
    const pre = document.getElementById("preauth");
    if (pre) { pre.hidden = true; }
    const app = document.getElementById("main");
    if (app) { app.hidden = false; }
    announce("Paired. Loading captured events.");
    try {
      const snap = await authedJson("/api/v1/events/snapshot", {});
      ingestSnapshot(snap, false);
    } catch (err) {
      if (err && err.authLoss) { onAuthLoss(); return; }
      enterDisconnected("ConnectionLost");
      return;
    }
    scheduleFollow();
  }

  /* ================= boot ================= */

  function disablePairEntry() {
    if (tokenInput) { tokenInput.value = ""; tokenInput.disabled = true; }
    if (pairButton) { pairButton.disabled = true; }
  }

  async function proveSwAbsentAndEnable() {
    if (!tokenInput || !pairButton) { return; }
    // Trusted shell code refuses token entry unless the ABSENCE of any
    // service-worker controller/registration is affirmatively proved.
    // Enumeration failure or API unavailability is not proof of absence.
    let swProof = "unproven";
    if ("serviceWorker" in navigator) {
      try {
        const controlled = Boolean(navigator.serviceWorker.controller);
        const regs = await navigator.serviceWorker.getRegistrations();
        swProof = (controlled || (regs && regs.length > 0)) ? "present" : "absent";
      } catch (_e) {
        swProof = "unproven";
      }
    }
    if (swProof !== "absent") {
      disablePairEntry();
      setPairError(swProof === "present"
        ? "A service worker is registered for this origin. Token entry is disabled. Launch the dashboard on a fresh origin."
        : "Service worker state could not be verified. Token entry is disabled. Launch the dashboard on a fresh origin.");
      return;
    }
    setPairError("");
    tokenInput.disabled = false;
    pairButton.disabled = false;
  }

  function boot() {
    if (!tokenInput || !pairButton) { return; }
    pairButton.addEventListener("click", attemptPair);
    tokenInput.addEventListener("keydown", (kev) => {
      if (kev.key === "Enter") { kev.preventDefault(); attemptPair(); }
    });
    proveSwAbsentAndEnable();
  }

  boot();
})();
