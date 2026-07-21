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

Create two connected local full-duplex channels: one for control/pairing and one for typed
inspection IPC. Pass only the intended child endpoints to the hidden child mode of the same
installed host executable. Close unintended copies.

The hidden child constructs ChildInheritedHandles and calls DashboardChildEntrypoint::run. The
entrypoint rejects non-socket handles and verifies no-dump readiness before binding or generating
sensitive state.

## 3. Complete pairing

Wrap the process and parent endpoints with SpawnedDashboardChild, then call
DashboardSupervisor::prepare. Supply a PairingDelivery implementation that writes the canonical
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
operation. Move only the returned ActivatedInspectionConsumerV1 into
PendingDashboardActivation::commit.

Do not expose or retain another sink, choose an epoch, inject a loose control object, or start
provider traffic before commit succeeds. If any post-install step fails, disable the activated
consumer and fully terminate/reap the dashboard child before continuing provider operation.

## 5. Operate and stop

Use DashboardControl::purge for reusable registration-bound purge. rotate_pairing_secret requires a
fresh acknowledged delivery and invalidates the previous authentication generation. shutdown is
one-way and returns only after disable, zeroization, termination, and reap.

Treat DashboardStatus::Disabled as a dashboard-only failure. Do not retry capture in the same
launch and do not alter the provider enforcement result.

## Environment prerequisites

The current child implementation requires connected Unix-domain sockets and a Unix resource-limit
API that can set and verify both core-dump limits at zero. Unsupported platforms fail closed for
dashboard activation. There is no in-process or thread-only fallback for sensitive state.
