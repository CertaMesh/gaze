# Dashboard trust boundary

Enabling the dashboard expands the local trusted computing base. By default
there is no dashboard entropy call, credential, listener, consumer, process,
store, or browser surface.

## Process boundary

| Provider owns | Child owns |
| --- | --- |
| Bounded nonblocking ingress; dedicated IPC writer/supervisor; one-shot registration binding; capped zeroizing in-flight frames; killable child handle | Literal-loopback listener; launch credential; page sessions; CSRF state; retained events; reveals; response buffers |

Provider request, enforcement, and restore paths never write dashboard IPC,
wait for dashboard work, join threads, terminate, or reap the child.

Before binding, generating secrets, reporting readiness, or accepting sensitive
frames, the child installs and verifies crash-dump suppression. It covers
non-Darwin Unix core limits. Darwin returns `Unsupported` and fails closed
before binding or secret generation; provider operation continues. No macOS
crash-artifact suppression is claimed.

## Capture authority

`ProviderVisible` is confidential pseudonymized content, with no verified-clean
claim. `OwnerRaw` and `OwnerRestored` each require startup selection and
acknowledgement. Browser reveals require an exact retained logical ID, stage,
emission ID, and domain. Browsers cannot promote capture, select epochs,
replace sinks, or revive disabled registrations.

No pending consumer exists until delivery of the 59-byte pairing frame and
matching 22-byte nonce acknowledgement. `gaze-inspection` can atomically install
the consumer and producer. Its one-shot `InspectionConsumerBindingV1` identifies
the registration; descriptors, caller trust, wrappers, and the activated handle
do not expose or distinguish descriptor-equal registration identities.

`PendingDashboardActivation::commit` checks the activated consumer against that
binding before socket, writer, runtime, or admission side effects. A different
registration returns `ActivationFailed`.

## Purge and fatal failure

```mermaid
flowchart LR
    C[Close admission] --> D[Drain + zeroize ingress]
    D --> G[begin_purge on bound consumer]
    G --> P[Purge child under exact guard]
    P --> A[Accept acknowledgement for guard epoch]
    A --> F[Complete matching guard]
    F --> O[Reopen for completed epoch]
```

Child purge zeroizes store, authentication, reveal permits, active responses,
and buffers while the guard is held. The runtime chooses the epoch.

Fatal child exit, IPC/deadline/writer faults, control-channel closure, or failed
rotation/purge wins over ordinary work. The supervisor disables the exact
consumer, zeroizes parent frames, terminates and reaps the child. Late commands
or acknowledgements cannot leave Disabled. Provider enforcement and restore
continue independently.

## Memory and revocation limits

Memory-only retention caps logical events, bytes, TTL, ingress, frames, page
sessions, followers, and active responses. TTL uses monotonic time; access does
not refresh it.

Response authority binds authentication generation, inspection epoch, logical
ID, stage, emission ID, domain, insertion generation, and deadline. One registered
zeroizing envelope holds the `GZPL` header and payload. Its reservation includes
lease and bounded write overhead; store plus responses cannot exceed the byte cap.

Purge, expiry, rotation, conceal, auth loss, disconnect, fatal failure, and
shutdown cancel later application writes and zeroize owned buffers. They cannot
revoke bytes already in a browser, OS network buffer, terminal scrollback,
extension, screenshot, or privileged memory capture. The host OS, controlling
terminal, and authenticated browser are trusted. Malicious trusted code and
privileged external capture are outside containment.

## Closed information limits

Unavailable queue snapshots must never appear as zero, healthy, empty, clean,
or no traffic. `ProjectionFailedClosed` stays one coarse caution.
Configured port metadata is categorical; never invent a numeric port, host,
URL, discovery path, or provenance.

`MetadataOnly` has no content-derived projection. Missing byte/chunk measurements,
JSON shape, PII summaries, SSE timelines, decisions, or attestations retain their
exact closed omission reason; absence never becomes zero, empty, or clean.

Each SSE entry has only ordinal, event kind, optional delta kind, and optional
content-block index. Rust and browser consumers must not derive per-entry bytes
or timing.
