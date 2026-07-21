# gaze-proxy-dashboard

gaze-proxy-dashboard is the provider-neutral, memory-only inspection dashboard runtime for Gaze.
It is deliberately absent by default. An adopter must explicitly construct the dashboard child,
complete acknowledged local pairing, create the pending consumer, atomically install that consumer
with the producer through gaze-inspection, and move the returned activated consumer back into the
dashboard.

Among Gaze crates, the normal dependency closure is exactly:

- gaze-types for closed, bounded value contracts;
- gaze-inspection for one-shot installation and registration-bound purge/disable authority.

The crate never depends on gaze-proxy, a provider adapter, a private transport, persistence, an
outbound client, analytics, telemetry, or a crash-dump handler.

## Security shape

- Dashboard-off constructs no token, sink, listener, child, store, or capture registration.
- Dashboard-on always authorizes ProviderVisible. OwnerRaw and OwnerRestored each require their own
  launch-time acknowledgement type.
- No browser request can promote capture.
- Pairing uses a 59-byte child frame, a 22-byte nonce-bound acknowledgement, and the one canonical
  43-byte unpadded base64url launch credential.
- The provider process performs one bounded nonblocking ingress send. A dedicated non-request
  writer owns typed, capped IPC.
- Listener, launch/session/CSRF authentication, retention, reveal state, and response buffers live
  in a killable child process.
- The child verifies application core-dump suppression before binding, token generation, or
  sensitive IPC acceptance. An unsupported or unverifiable platform disables dashboard activation.
- Purge ordering is fixed: close Track B admission, drain ingress, begin the registration-bound
  purge, zeroize child store/auth/reveal/response state while holding the matching guard, then
  complete the guard and reopen only for its returned epoch.
- Fatal disable is one-way and always terminates and reaps the sensitive child.

## Current typed limitations

The queue snapshot field is not measured and must be presented as unavailable, never as zero,
empty, healthy, or no traffic. ProjectionFailedClosed is intentionally coarse and must not be
expanded into a guessed cause. Configured ports are category-only and must never become numeric
ports, hosts, URLs, or provenance. MetadataOnly supplies no content-derived measurements,
structure, PII, timeline, decision, or attestation projection. Every absent projection carries its
exact closed omission reason.

SSE timeline entries expose only ordinal, event kind, optional delta kind, and optional
content-block index. They contain no per-entry byte count, timestamp, latency, cadence, or relative
timing.

See the dashboard [trust boundary](../../docs/explanation/dashboard/trust-boundary.md), [local
operation guide](../../docs/how-to/dashboard/run-local-dashboard.md), and [browser security
reference](../../docs/reference/dashboard/browser-security.md).
