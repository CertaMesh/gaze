# Dashboard trust boundary

The dashboard expands the local trusted computing base only when an adopter explicitly enables it.
The default is absence: no dashboard entropy call, credential, listener, inspection consumer,
process, store, or browser surface exists.

## Process boundary

The provider process retains only bounded, nonblocking inspection ingress, a dedicated non-request
IPC writer/supervisor, a future registration-identity receipt, capped zeroizing in-flight frames,
and the killable child handle. Provider request, enforcement, and restoration paths never perform
dashboard IPC writes, wait for dashboard work, join threads, terminate the child, or reap it.

The child owns every sensitive dashboard concern: the literal-loopback listener, launch
credential, page sessions, CSRF state, retained events, reveals, and response buffers. Before it
binds, generates a credential, declares readiness, or accepts a sensitive frame, it must install
and verify application crash-dump suppression. The reviewed implementation currently covers
non-Darwin Unix core limits. Darwin reports `Unsupported` and fails closed before binding or secret
generation; no macOS crash-artifact suppression is claimed. Provider operation continues.

## Capture authority

ProviderVisible is the dashboard-on baseline and is confidential pseudonymized content, not a
verified-clean result. OwnerRaw and OwnerRestored are separately selected and acknowledged at
startup. A browser can reveal only an exact retained logical ID, stage, emission ID, and domain that
was already captured. It cannot promote capture, choose an epoch, replace an inspection sink, or
revive a disabled registration.

The pending consumer does not exist until the 59-byte pairing frame has been delivered and the
matching 22-byte nonce acknowledgement has completed. Master composition can atomically install
the pending consumer with the producer through gaze-inspection, but the returned activated handle
exposes no registration identity. Dashboard activation therefore remains fail-closed. The minimal
sound completion is an opaque identity receipt/match operation in gaze-inspection; descriptor
equality, caller trust, and post-install wrappers cannot distinguish descriptor-equal registrations.

## Purge and fatal failure

After that identity authority exists, purge is serialized:

1. close dashboard admission;
2. drain and zeroize bounded ingress;
3. call begin_purge on the activated consumer;
4. while holding that exact guard, purge and zeroize child store, authentication, reveal permits,
   active-response state, and buffers;
5. accept only the child acknowledgement for the guard's runtime-selected epoch;
6. complete the matching guard;
7. reopen admission only for that completed epoch.

A fatal child exit, IPC fault, deadline, writer fault, control-channel closure, failed rotation, or
failed purge wins over ordinary work. The supervisor disables the exact activated consumer,
zeroizes parent frames, terminates the child, and reaps it. No late command or acknowledgement can
leave Disabled. Provider PII enforcement and restoration continue independently.

## Memory and revocation limits

Retention is memory-only, capped by logical-event, byte, TTL, ingress, frame, page-session,
follower, and active-response limits. TTL uses a monotonic clock and access never refreshes it.
Response authorization is bound to authentication generation, inspection epoch, logical ID, stage,
emission ID, domain, insertion generation, and deadline. One registered zeroizing response envelope
contains the `GZPL` header and payload; its full reservation includes lease and bounded write
overhead, so store plus concurrent responses cannot exceed the configured byte cap.

Purge, expiry, rotation, conceal, authentication loss, disconnect, fatal failure, and shutdown
cancel later application writes and zeroize owned buffers. Bytes already delivered to an
authenticated browser, operating-system network buffer, terminal scrollback, extension,
screenshot, or privileged memory-capture facility cannot be revoked. The host operating system,
controlling terminal, and authenticated browser are inside the owner trust boundary; malicious
trusted code and privileged external capture are outside the containment claim.

## Closed information limits

Queue snapshots are unavailable/not measured. They must not render as zero, healthy, empty, clean,
or no traffic. ProjectionFailedClosed remains one coarse caution label. Configured port metadata
is a category only; a numeric port, host, URL, discovery path, or provenance must never be invented.

MetadataOnly contains no content-derived projection. Missing byte/chunk measurements, JSON shape,
PII summaries, SSE timelines, decision traces, or attestation traces carry and render their exact
closed omission reason. An absent projection never becomes zero, an empty collection, or a clean
claim.

SSE entries contain exactly ordinal, event kind, optional delta kind, and optional content-block
index. They contain no per-entry bytes or timing, and neither the Rust view model nor browser
consumer may derive those values.
