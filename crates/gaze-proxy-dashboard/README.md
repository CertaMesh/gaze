# gaze-proxy-dashboard

gaze-proxy-dashboard is the provider-neutral, memory-only inspection dashboard runtime for Gaze.
It is deliberately absent by default. An adopter must explicitly construct the dashboard child,
complete acknowledged local pairing, and create the pending consumer. Activation currently fails
closed because `ActivatedInspectionConsumerV1` exposes no unforgeable registration identity. The
remaining fix belongs in `gaze-inspection`; descriptor equality and caller assertions are not an
acceptable substitute.

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
- On reviewed non-Darwin Unix targets, the child verifies zero core-dump limits before binding,
  token generation, or sensitive IPC acceptance. Darwin is explicitly unsupported and fails closed
  before those operations; the implementation makes no macOS crash-artifact suppression claim.
- Parent channels are crate-owned: `SpawnedDashboardChild::spawn` creates private sockets, launches
  the exact child, and validates the peer PID where the OS exposes it. There is no public loose
  `Child` plus `UnixStream` assembly API.
- Inspection EOF, partial framing, oversize framing, or decode failure purges child state, stops
  HTTP/control service, exits the child, and causes parent disable/reap.
- Once the identity blocker is resolved, purge ordering is fixed: close Track B admission, drain ingress, begin the registration-bound
  purge, zeroize child store/auth/reveal/response state while holding the matching guard, then
  complete the guard and reopen only for its returned epoch.
- Fatal disable is one-way and always terminates and reaps the sensitive child.

## Current typed limitations

Dashboard activation is intentionally unavailable on the current dependency manifest. A sound
commit requires authority to add an opaque registration receipt/match operation in
`crates/gaze-inspection/src/lib.rs` and its UI/compile-fail tests. The dashboard's `commit` method
disables the supplied handle, tears down the child, and returns `ActivationFailed` until then.

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
