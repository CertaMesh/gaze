# Dashboard accessibility and visual verification

Rendered verification record for the dashboard's 44-state matrix and
frontend/XSS/accessibility contract. The browser suite regenerates the
[per-assertion ledger](../../../crates/gaze-proxy-dashboard/browser-tests/evidence/state-ledger.json).

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

## Run identity

Historical verification used Playwright Chromium, axe-core and
`crates/gaze-proxy-dashboard/tests/browser_contract.rs`. Assets: `0886177`;
harness: `a46886f`. The recorded result was 63/63 tests: 44 matrix states plus
19 security/lifecycle/accessibility suites, including four post-review regressions.

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

Each state ran under production security headers and passed every recorded
assertion. Screenshots support human review.

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

| Check | Recorded result |
|---|---|
| axe-core | Zero serious/critical violations in shell and paired UI. Set-position attributes live on list items, not buttons. |
| Text contrast | Light ≥5.75:1; dark ≥6.70:1; caution 6.62–7.80:1. AA text floor: 4.5:1. |
| Focus | Sampled rectangles stay visible and avoid the safety bar; scroll padding is zero when the bar is non-sticky. |
| Safety bar | Non-sticky at viewport height ≤400 CSS px. |
| Targets | Buttons ≥24×24 px; Purge checked at V9. |
| Reflow | No horizontal document overflow, including 320 px. Wide tables scroll internally. |
| Reduced motion | All animation/transition durations 0s. |
| Forced colors | Double/solid/dashed lane borders and labels preserve meaning. Glyphs are `aria-hidden`. |
| Text spacing | WCAG overrides cause no clipping or overflow. |
| Dark mode | OS preference only; no stored toggle. |
| Announcements | One bounded `role=status` for reveal/conceal/expiry/purge/follow/session end; auth errors use `role=alert`. Payloads are never live regions. |
| Authentication | Paste-enabled password input; no puzzle, `name` or form; autofill disabled. |

### SC 2.2.1 (Timing Adjustable) — Essential exception claim

The 30-second owner reveal limit claims WCAG 2.2 SC 2.2.1's Essential exception:
longer display would extend re-identifiable PII exposure. Reauthorization starts
a separately confirmed window. Expiry always announces itself; concealment
returns focus to the reveal control when it removes the focused region.

## Security and leakage results

- The Rust asset contract forbids injection sinks, `eval`, string timers,
  `EventSource`, storage APIs, SVG, frames and external links.
- Suites use production CSP and Trusted Types. Only axe injection relaxes
  `require-trusted-types-for 'script'; trusted-types 'none'`.
- The 43-character launch-token canary appears only in the pair request's
  `Authorization: GazeDashboardV1 <43>` header, never DOM, attributes, console,
  URLs, storage or page errors. Fetch uses `credentials: omit`, `cache: no-store`,
  `redirect: error`, `referrerPolicy: no-referrer`.
- Owner sentinels exist only in text nodes during reveal. Conceal, expiry,
  navigation, lifecycle clearing and terminal states remove their bytes. Safe
  snapshot/follow responses contain none.
- Hostile markup, prototype keys, bidi/control/zero-width characters render as
  inert LTR text with `⟦U+XXXX⟧` escapes; no script, dialog or prototype mutation.
- Cookies, local/session storage, Cache API and service-worker registrations
  stay empty. Token entry requires proof that no worker controls or registers
  on the origin; enumeration failure or unavailable APIs keep it disabled.
- SSE rows expose only ordinal, event kind, delta kind and content-block index.
  No byte counts, timestamps, cadence, latency or relative time.
- Queue telemetry says `UNAVAILABLE — NOT MEASURED`. Missing projections use
  exact omission reasons; `ProjectionFailedClosed` stays coarse. Ports show
  categories only. Zero events do not prove no traffic. No success style exists.
- Dashboard disconnect/disable purges data and leaves the proxy unaffected.

See [browser security](browser-security.md) for the runtime rules.

## Accepted limitations and conditionals (honest record)

| Limitation | Required follow-up or scope |
|---|---|
| VoiceOver + Safari | Not performed; human macOS pass required each release. |
| NVDA + Windows high contrast | Not performed; human Windows pass required each release. |
| Browser/OS credential-store probe | Not performed. Headless Chromium checks input attributes, not save prompts. |
| axe CSP | Trusted Types relaxed only for axe injection. |
| Zoom | V8/V9 use effective CSS viewports, not browser-chrome zoom. |
| Pixel review | Implementer checked geometry, styles, contrast and ARIA; 45 synthetic screenshots were stored for independent visual review. |
| Paragraph spacing | Applied per paragraph with CSSOM; other spacing inherits from root. |

## Interface needs recorded for core/master reconciliation

The dev fixture mocks the typed API. These fixture-owned wire details require
core integration reconciliation:

1. Bootstrap envelope: fixture uses 70 bytes: `GZDB` ‖ `0x01` ‖ `0x02` ‖
   32-byte page-session secret ‖ 32-byte CSRF secret. The browser decodes
   only this fixed shape and rejects any other length/magic/version.
2. Secondary-secret headers: authed calls send
   `x-gaze-page-session` and `x-gaze-csrf` as 43-char unpadded base64url.
   The launch credential is dropped from page memory after pairing.
3. Payload envelope: fixture uses `GZPL` ‖ `0x01` ‖ domain tag (1–3) ‖
   stage tag (1–4) ‖ u32be length ‖ UTF-8 text, hard-capped at 4 MiB in the
   browser decoder, rendered exclusively as text nodes.
4. Safe-metadata JSON shape: `runtime` (lifecycle/captureTier/ttl/ring/
   epoch), `counters` (distinct saturating drop counters), `queue: null`
   (placeholder limitation), and `events[]` view models mirroring the closed
   vocabulary with `{state: Present|Omitted, reason}`
   availability wrappers. Field names are fixture-owned; the closed-code
   string values are the frozen spellings.
5. Disabled-code vocabulary: the UI maps a closed set
   (`ChildExit`, `IpcFault`, `PurgeTimeout`, `Rotation`, `Shutdown`,
   `ConnectionLost`, `UnknownFuture`) to neutral labels; unknown codes fall
   closed to `UNKNOWN`.
6. Follow transport: polling POST returning the full safe snapshot; the
   client diffs logical IDs for the buffered-count pause/resume contract.
   NDJSON streaming would slot into the same ingest path.

None of these stand-ins add provider semantics, reconstruct projections, or
narrow the 44-state matrix; renderers treat every unknown wire value as a
closed caution state.

## Post-review corrections

Four tests raised the historical suite from 59 to 63; the 44 states and 259
matrix assertions stayed unchanged.

- `LC-SCROLL-PAUSE`: real upward window scrolling pauses follow at V7;
  downward scrolling stays live, paused rows stay fixed, Resume applies buffers.
  The listener also supports a future element scroller.
- `SEC-SW-FAILCLOSED`: worker enumeration failure or unavailable APIs disable
  token entry.
- `LC-PREAUTH-HIDDEN` and `LC-PREAUTH-ABORT`: hidden visibility, freeze and
  pagehide clear/disable the preauth token and abort pairing; becoming visible
  reruns worker proof before enabling input.

These regressions failed on the pre-fix assets.
