# Run the local dashboard

The dashboard is an explicit adopter composition. Do not construct any dashboard object on the
default/off path.

## 1. Select immutable startup capture

Create DashboardPayloadAcceptance::provider_visible() for the baseline. Add OwnerRaw only with
OwnerRawRiskAcknowledgement::acknowledge_pii_risk(). Add OwnerRestored only with
OwnerRestoredRiskAcknowledgement::acknowledge_reidentification_risk().

These selections are immutable for the launch. A browser request cannot add a domain later.

Use LoopbackBind::fresh_ephemeral_v4() for a fresh literal address in 127.0.0.0/8 and operating
system port zero. A configured literal loopback address must still use port zero and should display
an origin-reuse warning.

## 2. Spawn the sensitive child

Pass the hidden child command to `SpawnedDashboardChild::spawn`. The crate creates a private 0700
socket directory, owns both control/inspection listeners, injects only their paths into the exact
child launch, and validates peer PID credentials where the operating system exposes them. There is
no public constructor accepting a loose child and unrelated channel pair.

The hidden child calls `ChildInheritedHandles::connect_from_environment`, then
`DashboardChildEntrypoint::run`. The entrypoint rejects non-socket handles and verifies no-dump
readiness before binding or generating sensitive state.

## 3. Complete pairing

Pass the returned `SpawnedDashboardChild` to `DashboardSupervisor::prepare`. Supply a
`PairingDelivery` implementation that writes the canonical
43-byte credential only to a controlling terminal or another reviewed acknowledged local channel.
Never place it in arguments, environment variables, stdout/stderr logs, files, URLs, cookies,
HTML, browser storage, or telemetry.

Preparation returns PairedDashboard only after the child frame and nonce-bound delivery
acknowledgement complete.

## 4. Atomically install inspection

Consume PairedDashboard::into_pending_activation() to receive:

- one PendingDashboardActivation;
- one provider-neutral PendingInspectionConsumerV1;
- the exact immutable DashboardCaptureDescriptorV1.

Pass the pending consumer and the producer half to the one atomic gaze-inspection installation
operation. On the current API, do not start provider traffic: `ActivatedInspectionConsumerV1`
does not expose an unforgeable identity that the pending dashboard half can match. Consequently,
`PendingDashboardActivation::commit` disables the handle, tears down the child, and returns
`ActivationFailed`.

Activation requires a new opaque registration receipt/match operation in gaze-inspection plus its
compile-fail/UI tests. Do not substitute descriptor equality, a caller assertion, a generic
closure, or a wrapper created after installation.

Do not expose or retain another sink, choose an epoch, inject a loose control object, or start
provider traffic before commit succeeds. If any post-install step fails, disable the activated
consumer and fully terminate/reap the dashboard child before continuing provider operation.

## 5. Operate and stop

After the gaze-inspection identity API exists, use `DashboardControl::purge` for reusable
registration-bound purge. `rotate_pairing_secret` requires a
fresh acknowledged delivery and invalidates the previous authentication generation. shutdown is
one-way and returns only after disable, zeroization, termination, and reap.

Treat DashboardStatus::Disabled as a dashboard-only failure. Do not retry capture in the same
launch and do not alter the provider enforcement result.

## Environment prerequisites

The current child implementation requires Unix-domain sockets and a reviewed Unix resource-limit
API that can set and verify both core-dump limits at zero. Darwin is explicitly unsupported and
returns `NoDumpUnavailable` before binding, token generation, or sensitive IPC acceptance. No
macOS crash-artifact suppression is claimed. There is no in-process or thread-only fallback.
