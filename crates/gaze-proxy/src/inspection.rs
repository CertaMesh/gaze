//! Provider-neutral proxy inspection producer and projection ownership.
//!
//! Queueing, lifecycle fencing, sink delivery, purge, disable, and sensitive wrapper ownership
//! remain in `gaze-inspection`. This module only installs the proxy's pending producer half and
//! projects bytes/facts already owned by the validated direct path. Inspection outcomes are
//! deliberately ignored by enforcement callers.

use gaze_inspection::{
    install_inspection_v1, ActivatedInspectionConsumerV1, InspectionInstallErrorV1,
    InspectionLogicalEmitterV1, InstalledInspectionProducerV1, OwnerRawPayloadV1,
    OwnerRawProjectionV1, OwnerRestoredPayloadV1, OwnerRestoredProjectionV1,
    PendingInspectionConsumerV1, PendingInspectionProducerV1, ProviderVisiblePayloadV1,
    ProviderVisibleProjectionV1,
};
use gaze_types::inspection::{
    CoarseDurationBucketV1, DashboardCaptureDescriptorV1, EndpointSelectionCodeV1,
    InspectionDeliveryCodeV1, InspectionDropCodeV1, InspectionErrorCodeV1, InspectionMeasurementV1,
    InspectionOperationalStatusV1, InspectionQueueSnapshotV1, InspectionSafeFieldsV1,
    PortSelectionCodeV1, ProjectionAvailabilityV1, ProjectionOmissionReasonV1,
    ProviderProfileCodeV1, RouteCodeV1,
};

use crate::codec::{InspectionProjectionSourceV1, InspectionStructuralProjectionV1};

/// Opaque installed proxy producer created only by the atomic matched installation operation.
///
/// It exposes no sink, descriptor, registration identity, epoch, lifecycle control, or delivery
/// internals. The activated consumer returned beside it remains the adopter's sole post-install
/// purge/disable authority.
#[must_use]
pub struct ProxyInspectionProducerV1 {
    producer: InstalledInspectionProducerV1,
}

/// Atomically installs the proxy producer against an adopter-supplied pending consumer half.
///
/// The exact immutable descriptor is used to create the producer pending half. Descriptor
/// mismatch, dispatcher failure, and registration exhaustion remain closed installation errors.
pub fn install_proxy_inspection_v1(
    descriptor: DashboardCaptureDescriptorV1,
    consumer: PendingInspectionConsumerV1,
) -> Result<(ProxyInspectionProducerV1, ActivatedInspectionConsumerV1), InspectionInstallErrorV1> {
    let producer = PendingInspectionProducerV1::new(descriptor);
    let (producer, consumer) = install_inspection_v1(producer, consumer)?;
    Ok((ProxyInspectionProducerV1 { producer }, consumer))
}

impl ProxyInspectionProducerV1 {
    pub(crate) fn begin_logical(&self) -> Option<ProxyInspectionLogicalV1> {
        self.producer
            .begin_logical()
            .ok()
            .map(|emitter| ProxyInspectionLogicalV1 { emitter })
    }
}

pub(crate) struct ProxyInspectionLogicalV1 {
    emitter: InspectionLogicalEmitterV1,
}

impl ProxyInspectionLogicalV1 {
    pub(crate) fn emit_request_stages(
        &mut self,
        owner_raw: &[u8],
        provider_visible: &[u8],
        source: Option<&dyn InspectionProjectionSourceV1>,
    ) -> [gaze_inspection::InspectionAdmissionOutcomeV1; 2] {
        let owner = self.emitter.try_emit_owner_request(safe_fields(), || {
            ProjectionAvailabilityV1::Present(owner_raw_projection(owner_raw, source))
        });
        let provider = self.emitter.try_emit_provider_request(safe_fields(), || {
            ProjectionAvailabilityV1::Present(provider_visible_projection(provider_visible, source))
        });
        [owner, provider]
    }

    pub(crate) fn emit_response_stages(
        &mut self,
        provider_visible: &[u8],
        owner_restored: &[u8],
        source: Option<&dyn InspectionProjectionSourceV1>,
    ) -> [gaze_inspection::InspectionAdmissionOutcomeV1; 2] {
        let provider = self.emitter.try_emit_provider_response(safe_fields(), || {
            ProjectionAvailabilityV1::Present(provider_visible_projection(provider_visible, source))
        });
        let owner = self
            .emitter
            .try_emit_owner_restored_response(safe_fields(), || {
                ProjectionAvailabilityV1::Present(owner_restored_projection(owner_restored, source))
            });
        [provider, owner]
    }
}

fn safe_fields() -> InspectionSafeFieldsV1 {
    InspectionSafeFieldsV1::new(
        RouteCodeV1::AnthropicMessages,
        ProviderProfileCodeV1::AnthropicMessagesDirect,
        EndpointSelectionCodeV1::ConfiguredHttpsOrigin,
        PortSelectionCodeV1::NotApplicable,
        InspectionOperationalStatusV1::Accepted,
        InspectionErrorCodeV1::None,
        InspectionDropCodeV1::None,
        InspectionDeliveryCodeV1::Pending,
        CoarseDurationBucketV1::NotMeasured,
        InspectionQueueSnapshotV1::default(),
    )
}

fn owner_raw_projection(
    bytes: &[u8],
    source: Option<&dyn InspectionProjectionSourceV1>,
) -> OwnerRawProjectionV1 {
    let structure = structural_projection(source);
    OwnerRawProjectionV1::new(
        ProjectionAvailabilityV1::Present(OwnerRawPayloadV1::capture(bytes.to_vec())),
        measurement(bytes),
        structure.json_shape,
        structure.sse_timeline,
        unavailable(),
        unavailable(),
        unavailable(),
    )
}

fn provider_visible_projection(
    bytes: &[u8],
    source: Option<&dyn InspectionProjectionSourceV1>,
) -> ProviderVisibleProjectionV1 {
    let structure = structural_projection(source);
    ProviderVisibleProjectionV1::new(
        ProjectionAvailabilityV1::Present(ProviderVisiblePayloadV1::capture(bytes.to_vec())),
        measurement(bytes),
        structure.json_shape,
        structure.sse_timeline,
        unavailable(),
        unavailable(),
        unavailable(),
    )
}

fn owner_restored_projection(
    bytes: &[u8],
    source: Option<&dyn InspectionProjectionSourceV1>,
) -> OwnerRestoredProjectionV1 {
    let structure = structural_projection(source);
    OwnerRestoredProjectionV1::new(
        ProjectionAvailabilityV1::Present(OwnerRestoredPayloadV1::capture(bytes.to_vec())),
        measurement(bytes),
        structure.json_shape,
        structure.sse_timeline,
        unavailable(),
        unavailable(),
        unavailable(),
    )
}

fn measurement(bytes: &[u8]) -> ProjectionAvailabilityV1<InspectionMeasurementV1> {
    ProjectionAvailabilityV1::Present(InspectionMeasurementV1::new(
        u64::try_from(bytes.len()).unwrap_or(u64::MAX),
        1,
    ))
}

fn structural_projection(
    source: Option<&dyn InspectionProjectionSourceV1>,
) -> InspectionStructuralProjectionV1 {
    source.map_or_else(InspectionStructuralProjectionV1::unsupported, |source| {
        source.project()
    })
}

fn unavailable<T>() -> ProjectionAvailabilityV1<T> {
    ProjectionAvailabilityV1::Omitted(ProjectionOmissionReasonV1::NotApplicable)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Condvar, Mutex};
    use std::time::{Duration, Instant};

    use gaze_inspection::{
        InspectionAdmissionOutcomeV1, InspectionEventV1, InspectionQueueLimitsV1, InspectionSink,
        InspectionSinkErrorV1, NoopInspectionSinkV1,
    };
    use gaze_types::inspection::CaptureDomainsV1;

    use super::*;

    struct CountingProjectionSource {
        calls: AtomicUsize,
    }

    impl InspectionProjectionSourceV1 for CountingProjectionSource {
        fn project(&self) -> InspectionStructuralProjectionV1 {
            self.calls.fetch_add(1, Ordering::SeqCst);
            InspectionStructuralProjectionV1::unsupported()
        }
    }

    #[derive(Default)]
    struct BlockingSink {
        entered: Mutex<bool>,
        entered_changed: Condvar,
        released: Mutex<bool>,
        released_changed: Condvar,
    }

    impl BlockingSink {
        fn wait_until_entered(&self) {
            let guard = self.entered.lock().unwrap();
            let (guard, timeout) = self
                .entered_changed
                .wait_timeout_while(guard, Duration::from_secs(2), |entered| !*entered)
                .unwrap();
            assert!(!timeout.timed_out());
            assert!(*guard);
        }

        fn release(&self) {
            *self.released.lock().unwrap() = true;
            self.released_changed.notify_all();
        }
    }

    impl InspectionSink for BlockingSink {
        fn try_emit(&self, _event: InspectionEventV1) -> Result<(), InspectionSinkErrorV1> {
            *self.entered.lock().unwrap() = true;
            self.entered_changed.notify_all();
            let guard = self.released.lock().unwrap();
            drop(
                self.released_changed
                    .wait_while(guard, |released| !*released)
                    .unwrap(),
            );
            Ok(())
        }
    }

    fn install_for_test(
        domains: CaptureDomainsV1,
        sink: Arc<dyn InspectionSink>,
        max_items: usize,
    ) -> (ProxyInspectionProducerV1, ActivatedInspectionConsumerV1) {
        let descriptor = DashboardCaptureDescriptorV1::new(domains);
        let pending = PendingInspectionConsumerV1::new(
            descriptor,
            sink,
            InspectionQueueLimitsV1::new(max_items, 4096).unwrap(),
        );
        install_proxy_inspection_v1(descriptor, pending).unwrap()
    }

    fn begin_for_test(producer: &ProxyInspectionProducerV1) -> ProxyInspectionLogicalV1 {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if let Some(logical) = producer.begin_logical() {
                return logical;
            }
            assert!(
                Instant::now() < deadline,
                "inspection begin stayed contended"
            );
            std::thread::yield_now();
        }
    }

    #[test]
    fn metadata_only_never_invokes_content_projection_source() {
        let source = CountingProjectionSource {
            calls: AtomicUsize::new(0),
        };
        let (producer, _consumer) = install_for_test(
            CaptureDomainsV1::MetadataOnly,
            Arc::new(NoopInspectionSinkV1),
            4,
        );
        let mut logical = begin_for_test(&producer);
        let outcomes = logical.emit_request_stages(
            b"synthetic owner bytes",
            b"synthetic provider bytes",
            Some(&source),
        );
        assert!(outcomes
            .iter()
            .all(|outcome| matches!(outcome, InspectionAdmissionOutcomeV1::Accepted { .. })));
        assert_eq!(source.calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn bounded_queue_reports_queue_full_without_blocking_enforcement() {
        let sink = Arc::new(BlockingSink::default());
        let (producer, _consumer) = install_for_test(CaptureDomainsV1::All, sink.clone(), 2);
        let mut logical = begin_for_test(&producer);
        let request = logical.emit_request_stages(b"owner", b"provider", None);
        assert!(request
            .iter()
            .all(|outcome| matches!(outcome, InspectionAdmissionOutcomeV1::Accepted { .. })));
        sink.wait_until_entered();
        let response = logical.emit_response_stages(b"provider", b"owner", None);
        sink.release();
        assert_eq!(
            response,
            [
                InspectionAdmissionOutcomeV1::Dropped(InspectionDropCodeV1::QueueFull),
                InspectionAdmissionOutcomeV1::Dropped(InspectionDropCodeV1::QueueFull),
            ]
        );
    }

    #[test]
    fn dropping_proxy_producer_reports_queue_closed_without_building_projection() {
        let source = CountingProjectionSource {
            calls: AtomicUsize::new(0),
        };
        let (producer, _consumer) =
            install_for_test(CaptureDomainsV1::All, Arc::new(NoopInspectionSinkV1), 4);
        let mut logical = begin_for_test(&producer);
        drop(producer);
        let outcomes = logical.emit_request_stages(b"owner", b"provider", Some(&source));
        assert_eq!(
            outcomes,
            [
                InspectionAdmissionOutcomeV1::Dropped(InspectionDropCodeV1::QueueClosed),
                InspectionAdmissionOutcomeV1::Dropped(InspectionDropCodeV1::QueueClosed),
            ]
        );
        assert_eq!(source.calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn purge_and_disable_report_closed_outcomes_without_building_projection() {
        let source = CountingProjectionSource {
            calls: AtomicUsize::new(0),
        };
        let (producer, mut consumer) =
            install_for_test(CaptureDomainsV1::All, Arc::new(NoopInspectionSinkV1), 4);
        let mut logical = begin_for_test(&producer);
        let purge = consumer.begin_purge().unwrap();
        assert_eq!(
            logical.emit_request_stages(b"owner", b"provider", Some(&source)),
            [
                InspectionAdmissionOutcomeV1::Dropped(InspectionDropCodeV1::Purging),
                InspectionAdmissionOutcomeV1::Dropped(InspectionDropCodeV1::Purging),
            ]
        );
        purge.complete().unwrap();
        consumer.disable();
        assert_eq!(
            logical.emit_response_stages(b"provider", b"owner", Some(&source)),
            [
                InspectionAdmissionOutcomeV1::Dropped(InspectionDropCodeV1::Disabled),
                InspectionAdmissionOutcomeV1::Dropped(InspectionDropCodeV1::Disabled),
            ]
        );
        assert_eq!(source.calls.load(Ordering::SeqCst), 0);
    }
}
