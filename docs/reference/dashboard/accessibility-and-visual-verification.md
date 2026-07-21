# Dashboard accessibility and visual verification

This document is the human-readable record of the rendered verification of the
`gaze-proxy-dashboard` frontend assets against the frozen 44-state visual
matrix (plan #4740 revision 60 §12) and the frontend/XSS/accessibility
contract (§11). The machine-readable per-assertion record is
[`crates/gaze-proxy-dashboard/browser-tests/evidence/state-ledger.json`](../../../crates/gaze-proxy-dashboard/browser-tests/evidence/state-ledger.json),
regenerated on every run of the browser contract.

## Run identity

- Implementer/verifier: visual/frontend worker, Solo process 4593
  (`track-b-visual-impl-4590`), Claude (Fable 5).
- Branch: `agent/proxy-dashboard-visual-4590`, based on frozen Track A
  `edc063761f6e21617d9fe3a5d414acb3e27d37de`
  (tree `9f7536fd5655cb9b04f6bfb1e815ebe800e206af`).
- Assets commit: `0886177` (`[agent] feat(dashboard): implement accessible
  inspection frontend`). Test-harness commit: `a46886f`.
- Toolchain: Playwright (Chromium) via the dev-only harness in
  `crates/gaze-proxy-dashboard/browser-tests/`; axe-core for the automated
  accessibility pass; a std-only Rust asset contract in
  `crates/gaze-proxy-dashboard/tests/browser_contract.rs`.
- Result: **63/63 automated tests pass** — the 44 matrix states plus 19
  security/lifecycle/accessibility suites (see "Post-review corrections"
  below for the four regression tests added after review).

## How to reproduce

```console
cd crates/gaze-proxy-dashboard/browser-tests
npm install
npx playwright install chromium
npx playwright test                # 44 states + security/lifecycle suites
node visual-audit.mjs              # quantitative rendered audit (geometry/contrast)
node serve.mjs                     # keep the fixture server up for manual review
```

Screenshots are human evidence, never pixel goldens; they are written outside
the repository (`GAZE_VISUAL_EVIDENCE_DIR`, defaulting to the OS temp dir) and
referenced by file name in the state ledger. They contain synthetic fixture
data only.

## Viewports

| ID | CSS viewport | Emulation | Layout proved |
|---|---|---|---|
| V1 | 1920×1080 | DPR 1 | P1 \| P2 side by side |
| V2 | 1440×900 | DPR 2 | reference side by side |
| V3 | 1280×800 | DPR 2 | compact side by side |
| V4 | 1024×768 | DPR 2, coarse pointer | side by side, narrowed P1 |
| V5 | 768×1024 | DPR 2, coarse pointer | stacked, P1 above P2 |
| V6 | 414×896 | DPR 3, coarse pointer | single column, P1/P2 exclusive |
| V7 | 360×640 | DPR 3, coarse pointer | single column, P1/P2 exclusive |
| V8 | 1280×800 at 200 % zoom | emulated at effective 640×400 | single-column reflow |
| V9 | 1280×1024 at 400 % zoom | emulated at effective 320×256 | single-column 320 px reflow |

Zoom states V8/V9 are emulated at their effective CSS viewport, which is the
layout-equivalent representation of browser zoom for reflow verification.

## The 44 rendered states

Every state below was rendered against the shipped assets under the full
production security-header set, asserted with the falsifiable checks listed in
the state ledger, and captured as a screenshot for human review. Assertion
counts are machine-verified PASS/FAIL checks; every state passed every
assertion.

| State | Viewport | Tier | Content | Condition | Fixture | Assertions | Result | Screenshot |
|---|---|---|---|---|---|---|---|---|
| L0-SHELL-V2 | V2 | preauth | shell | light | `fx-default` | 5 | PASS | `L0-SHELL-V2.png` |
| L0-SHELL-V7 | V7 | preauth | shell | light | `fx-default` | 5 | PASS | `L0-SHELL-V7.png` |
| L0-SHELL-V9 | V9 | preauth | shell | light | `fx-default` | 5 | PASS | `L0-SHELL-V9.png` |
| L0-AUTHERR-V2 | V2 | preauth | auth-error | light | `fx-default` | 4 | PASS | `L0-AUTHERR-V2.png` |
| L1-V1 | V1 | provider-visible-default | default | light-motion-normal | `fx-default` | 10 | PASS | `L1-V1.png` |
| L1-V2 | V2 | provider-visible-default | default | light-motion-normal | `fx-default` | 12 | PASS | `L1-V2.png` |
| L1-V3 | V3 | provider-visible-default | default | light-motion-normal | `fx-default` | 12 | PASS | `L1-V3.png` |
| L1-V4 | V4 | provider-visible-default | default | light-motion-normal | `fx-default` | 10 | PASS | `L1-V4.png` |
| L1-V5 | V5 | provider-visible-default | default | light-motion-normal | `fx-default` | 10 | PASS | `L1-V5.png` |
| L1-V6 | V6 | provider-visible-default | default | light-motion-normal | `fx-default` | 10 | PASS | `L1-V6.png` |
| L1-V7 | V7 | provider-visible-default | default | light-motion-normal | `fx-default` | 10 | PASS | `L1-V7.png` |
| L1-V8 | V8 | provider-visible-default | default | light-motion-normal | `fx-default` | 10 | PASS | `L1-V8.png` |
| L1-V9 | V9 | provider-visible-default | default | light-motion-normal | `fx-default` | 10 | PASS | `L1-V9.png` |
| L2-RAW-V2 | V2 | owner-raw | owner-tier | light | `fx-owner-raw` | 8 | PASS | `L2-RAW-V2.png` |
| L2-RAW-V9 | V9 | owner-raw | owner-tier | light | `fx-owner-raw` | 7 | PASS | `L2-RAW-V9.png` |
| L2-RESTORED-V2 | V2 | owner-restored | owner-tier | light | `fx-owner-restored` | 8 | PASS | `L2-RESTORED-V2.png` |
| L2-RESTORED-V9 | V9 | owner-restored | owner-tier | light | `fx-owner-restored` | 7 | PASS | `L2-RESTORED-V9.png` |
| L2-BOTH-V2 | V2 | owner-both | owner-tier | light | `fx-owner-both` | 8 | PASS | `L2-BOTH-V2.png` |
| L2-BOTH-V9 | V9 | owner-both | owner-tier | light | `fx-owner-both` | 9 | PASS | `L2-BOTH-V9.png` |
| L3-PVOMIT-V2 | V2 | content-pv-omitted | pv-omitted | light | `fx-content-pv-omitted` | 5 | PASS | `L3-PVOMIT-V2.png` |
| L3-OWNEROMIT-V2 | V2 | content-owner-omitted | owner-omitted | light | `fx-content-owner-omitted` | 3 | PASS | `L3-OWNEROMIT-V2.png` |
| L3-REVEALRAW-V2 | V2 | owner-raw | reveal-raw | light | `fx-owner-raw` | 6 | PASS | `L3-REVEALRAW-V2-revealed.png` |
| L3-REVEALRESTORED-V2 | V2 | owner-restored | reveal-restored | light | `fx-owner-restored` | 6 | PASS | `L3-REVEALRESTORED-V2-revealed.png` |
| L3-PVOMIT-V7 | V7 | content-pv-omitted | pv-omitted | light | `fx-content-pv-omitted` | 5 | PASS | `L3-PVOMIT-V7.png` |
| L3-OWNEROMIT-V7 | V7 | content-owner-omitted | owner-omitted | light | `fx-content-owner-omitted` | 3 | PASS | `L3-OWNEROMIT-V7.png` |
| L3-REVEALRAW-V7 | V7 | owner-raw | reveal-raw | light | `fx-owner-raw` | 6 | PASS | `L3-REVEALRAW-V7-revealed.png` |
| L3-REVEALRESTORED-V7 | V7 | owner-restored | reveal-restored | light | `fx-owner-restored` | 6 | PASS | `L3-REVEALRESTORED-V7-revealed.png` |
| L4-DARK-V2 | V2 | provider-visible-default | default-selected | dark | `fx-default` | 4 | PASS | `L4-DARK-V2.png` |
| L4-FORCED-V2 | V2 | provider-visible-default | default-selected | forced-colors | `fx-default` | 4 | PASS | `L4-FORCED-V2.png` |
| L4-REDUCED-V2 | V2 | provider-visible-default | default-selected | reduced-motion | `fx-default` | 4 | PASS | `L4-REDUCED-V2.png` |
| L4-TEXTSPACE-V2 | V2 | provider-visible-default | default-selected | text-spacing | `fx-default` | 4 | PASS | `L4-TEXTSPACE-V2.png` |
| L4-DARK-V9 | V9 | provider-visible-default | default-selected | dark | `fx-default` | 4 | PASS | `L4-DARK-V9.png` |
| L4-FORCED-V9 | V9 | provider-visible-default | default-selected | forced-colors | `fx-default` | 4 | PASS | `L4-FORCED-V9.png` |
| L4-REDUCED-V9 | V9 | provider-visible-default | default-selected | reduced-motion | `fx-default` | 4 | PASS | `L4-REDUCED-V9.png` |
| L4-TEXTSPACE-V9 | V9 | provider-visible-default | default-selected | text-spacing | `fx-default` | 4 | PASS | `L4-TEXTSPACE-V9.png` |
| L5-JSON-DEEP | V2 | provider-visible-default | json-depth-64 | light | `fx-structure-deep` | 4 | PASS | `L5-JSON-DEEP.png` |
| L5-JSON-WIDE | V2 | provider-visible-default | json-wide | light | `fx-structure-wide` | 3 | PASS | `L5-JSON-WIDE.png` |
| L5-JSON-MALFORMED | V2 | provider-visible-default | json-malformed | light | `fx-structure-malformed` | 2 | PASS | `L5-JSON-MALFORMED.png` |
| L5-SSE-10K | V2 | provider-visible-default | sse-10000 | light | `fx-sse-10k` | 5 | PASS | `L5-SSE-10K.png` |
| L5-DROPS | V2 | provider-visible-default | drops | light | `fx-drops` | 3 | PASS | `L5-DROPS.png` |
| L5-ZERO | V2 | provider-visible-default | zero-events | light | `fx-zero` | 2 | PASS | `L5-ZERO.png` |
| L5-DISCONNECTED | V2 | disabled | disconnected | light | `fx-disconnected` | 4 | PASS | `L5-DISCONNECTED.png` |
| L5-SHUTDOWN | V2 | shutdown | shutdown-purged | light | `fx-shutdown` | 2 | PASS | `L5-SHUTDOWN.png` |
| L5-PURGED | V2 | provider-visible-default | purged | light | `fx-purged` | 2 | PASS | `L5-PURGED.png` |

Fifteen additional non-matrix suites (`SEC-*`, `LC-*`, `A11Y-*`) prove the
security and lifecycle contract; they are recorded in the same ledger.

## Accessibility results

- **axe-core:** zero serious or critical violations on the preauth shell and
  the paired application (list + detail + lanes). One earlier finding
  (`aria-allowed-attr` from set-position attributes on buttons) was fixed by
  moving `aria-setsize`/`aria-posinset` to the list items.
- **Computed contrast (quantitative audit):** all sampled text/background
  pairs ≥ 5.75:1 in light scheme and ≥ 6.70:1 in dark scheme; caution
  surfaces 6.62–7.80:1. WCAG 2.2 AA floor is 4.5:1 for text.
- **Focus:** every sampled focus rectangle is visible inside the viewport and
  never intersects the sticky safety bar. The machine gate caught a real
  defect during implementation — the `scroll-padding-top` offset derived from
  the safety bar exceeded short viewports where the bar is non-sticky, which
  broke focus scrolling at V9 — and the fix (zero offset when the bar is not
  sticky) is regression-locked by the same gate.
- **Sticky bar:** at viewport height ≤ 400 CSS px the bar is non-sticky by
  stylesheet contract, so it can never obscure focus at V8/V9.
- **Targets:** all buttons have a 24×24 px minimum via the stylesheet; the
  Purge control is asserted ≥ 24×24 at the worst-case V9 state.
- **Reflow:** zero horizontal document overflow at every viewport including
  320 px (V9); wide tables scroll inside their own containers.
- **Reduced motion:** every element computes 0s animation and transition
  durations; screenshots show only the static presentation.
- **Forced colors:** lane border grammar (double/solid/dashed) and all text
  labels survive; meaning never relies on color alone. Lane glyphs are
  `aria-hidden`; the text labels carry the semantics.
- **Text spacing:** WCAG 2.2 letter/word/line/paragraph spacing overrides
  cause no clipping and no overflow.
- **Dark scheme:** OS-level only; there is no persisted toggle and no storage
  write.
- **Status messages:** reveal, conceal, expiry, purge, pause/resume, and
  session-end announcements use one bounded `role=status` region; the auth
  failure message uses `role=alert`. Payload regions are never live regions.
- **Accessible authentication:** pairing is a single paste-enabled password
  input with no cognitive puzzle, no name attribute, no form element, and
  autofill surfaces disabled.

### SC 2.2.1 (Timing Adjustable) — Essential exception claim

The 30-second owner-payload reveal window is a security limit on the exposure
of re-identifiable PII, claimed under the WCAG 2.2 SC 2.2.1 "Essential"
exception. Extending the window would extend the exposure of raw or restored
PII bytes in the DOM, which contradicts the product's core fail-closed
confidentiality contract; re-authorization starts a fresh, separately
confirmed window rather than extending the old one. Expiry is announced
unconditionally via the status region, and focus is returned to the reveal
control when concealment removes the focused region.

## Security and leakage results

- **Prohibited sinks:** the Rust asset contract pins the absence of every
  blocker-class sink (`innerHTML`, `insertAdjacentHTML`, `document.write`,
  `eval`, string timers, `EventSource`, storage APIs, SVG/frames/external
  links, and more) in the shipped assets.
- **CSP/Trusted Types:** all suites run under the exact production CSP
  including `require-trusted-types-for 'script'; trusted-types 'none'`; the
  application uses no injection sink and needs no policy. The only exception
  is the axe injection pass, which runs under an otherwise-identical CSP
  without the two Trusted Types directives, because axe itself cannot be
  injected under `trusted-types 'none'`. This is recorded as a conditional.
- **Token hygiene:** the synthetic 43-char launch-token canary appears in no
  DOM byte, attribute, console message, request URL, storage surface, or
  page error across every state; it is sent only as the canonical
  `Authorization: GazeDashboardV1 <43>` header on the pair request with
  `credentials: omit`, `cache: no-store`, `redirect: error`, and
  `referrerPolicy: no-referrer`.
- **Concealment is DOM byte absence:** owner payload sentinels are absent
  before reveal, present only inside text nodes during a reveal, and absent
  again after manual conceal, expiry, navigation, lifecycle clearing, and
  terminal states. Safe-metadata snapshot/follow responses never contain
  payload sentinels.
- **Hostile data:** markup, prototype-pollution-shaped keys, and
  bidi/control/zero-width characters render as inert sanitized text
  (`⟦U+XXXX⟧` placeholders in LTR plain text); no element or script is
  created, no dialog fires, and `Object.prototype` is unpolluted.
- **Fresh origin:** cookies, localStorage, sessionStorage, Cache API, and
  service-worker registrations are all empty after full use; the shell enables
  token entry only after affirmatively proving that no service worker controls
  or is registered for the origin — enumeration failure or API unavailability
  keeps entry disabled (fail closed).
- **SSE rule (seam audit #5953 INFO-2):** stream rows render ordinal, event
  kind, delta kind, and content-block index only. Table headers are asserted
  exactly; per-entry byte counts, timestamps, cadence, latency, and relative
  timing are asserted absent, and the accepted limitation is stated in the UI
  copy itself.
- **Closed-state honesty:** placeholder queue telemetry renders
  "QUEUE TELEMETRY: UNAVAILABLE — NOT MEASURED"; `ProjectionFailedClosed`
  renders its exact caution label; configured ports render category labels
  only (no numeric port anywhere); MetadataOnly-style absences render their
  exact closed omission reasons; the zero-event state explicitly disclaims
  being a no-traffic claim; no success or affirmative style token exists in
  the stylesheet (machine-asserted).
- **Provider continuity:** disabled/disconnected copy states that dashboard
  data was purged and the proxy is unaffected; wording that implies provider
  impairment is asserted absent.

## Accepted limitations and conditionals (honest record)

1. **Manual VoiceOver + Safari pass: NOT PERFORMED.** Requires a human
   operator on macOS. Plan §12 requires one per release; recorded as an open
   environment-prerequisite conditional.
2. **Manual NVDA + Windows high-contrast pass: NOT PERFORMED.** Requires a
   human operator on Windows; same conditional class.
3. **Browser/OS credential-store write probe: NOT PERFORMED.** Headless
   Chromium exposes no password-manager UI. The static contract
   (`type=password`, `autocomplete=off`, no `name`, no `form`) is
   machine-asserted; the behavioral no-save-prompt proof needs the manual
   browser matrix.
4. **axe ran under a Trusted-Types-relaxed CSP variant** (see above); every
   other automated check ran under the full production CSP.
5. **V8/V9 zoom emulated at effective CSS viewports** rather than through a
   browser-chrome zoom gesture; this is the standard layout-equivalent
   emulation for reflow testing.
6. **Screenshot pixel review:** the implementer verified rendering through
   DOM geometry, computed styles, quantitative contrast, aria snapshots, and
   the assertion roster, and stored all 45 screenshots for the independent
   rendered visual review required by the Track B phase gates. Screenshots
   contain synthetic fixture data only.
7. **Text-spacing paragraph margins** were applied per-paragraph via CSSOM in
   the test (letter/word/line spacing inherit from the root); this matches
   the WCAG test procedure's intent.

## Interface needs recorded for core/master reconciliation

The dev fixture server mocks only the closed typed API. The following wire
details are fixture-owned stand-ins that the core crate owns and must
reconcile at integration (recorded per the visual worker's brief; no runtime
file was touched):

1. **Bootstrap envelope:** fixture uses 70 bytes: `GZDB` ‖ `0x01` ‖ `0x02` ‖
   32-byte page-session secret ‖ 32-byte CSRF secret. The browser decodes
   only this fixed shape and rejects any other length/magic/version.
2. **Secondary-secret headers:** authed calls send
   `x-gaze-page-session` and `x-gaze-csrf` as 43-char unpadded base64url.
   The launch credential is dropped from page memory after pairing.
3. **Payload envelope:** fixture uses `GZPL` ‖ `0x01` ‖ domain tag (1–3) ‖
   stage tag (1–4) ‖ u32be length ‖ UTF-8 text, hard-capped at 4 MiB in the
   browser decoder, rendered exclusively as text nodes.
4. **Safe-metadata JSON shape:** `runtime` (lifecycle/captureTier/ttl/ring/
   epoch), `counters` (distinct saturating drop counters), `queue: null`
   (placeholder limitation), and `events[]` view models mirroring the closed
   #4732 revision 7 vocabulary with `{state: Present|Omitted, reason}`
   availability wrappers. Field names are fixture-owned; the closed-code
   string values are the frozen #4732 spellings.
5. **Disabled-code vocabulary:** the UI maps a closed set
   (`ChildExit`, `IpcFault`, `PurgeTimeout`, `Rotation`, `Shutdown`,
   `ConnectionLost`, `UnknownFuture`) to neutral labels; unknown codes fall
   closed to `UNKNOWN`.
6. **Follow transport:** polling POST returning the full safe snapshot; the
   client diffs logical IDs for the buffered-count pause/resume contract.
   NDJSON streaming would slot into the same ingest path.

None of these stand-ins add provider semantics, reconstruct projections, or
narrow the 44-state matrix; renderers treat every unknown wire value as a
closed caution state.

## Post-review corrections (2026-07-21, branch `agent/proxy-dashboard-visual-scroll-fix-4590`)

Two review findings against the visual surface were remediated and
regression-locked after the original 59-test evidence run:

1. **Scroll-up follow pause (#5971 BLOCKER-1):** the pause listener was bound
   to `.pane-list`, which is not a scroll container, so a real upward scroll
   of the document never paused live follow. Upward-scroll detection now also
   binds once to `window` (the actual rendered scroll container at every
   viewport); the element-level listener remains for any future element
   scroller. `LC-SCROLL-PAUSE` scrolls the real document with wheel input at
   V7 and proves: downward scroll stays LIVE, upward scroll shows
   `FOLLOW PAUSED — N BUFFERED`, rows never change while paused, and explicit
   Resume applies buffered rows. The test fails on the pre-fix assets.
2. **Service-worker proof and pre-auth token lifecycle (#5976 BLOCKER-7):**
   the boot proof previously treated enumeration failure as "clean" and the
   hidden-lifecycle teardown ran only post-auth. Token entry now enables only
   after affirmatively proving the absence of any service-worker controller
   and registration; enumeration rejection or API unavailability keeps entry
   disabled (fail closed). `visibilitychange(hidden)`, `freeze`, and
   `pagehide` clear and disable the pre-auth token input and abort any
   in-flight pairing request; returning visible re-runs the proof before
   re-enabling. `SEC-SW-FAILCLOSED`, `LC-PREAUTH-HIDDEN`, and
   `LC-PREAUTH-ABORT` prove each path in a real browser with token canaries;
   all three fail on the pre-fix assets.

The 44-state matrix, its 259 assertions, and the four honest conditionals are
unchanged; the suite total is now 63.
