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
    CaptureDomainsV1, CoarseDurationBucketV1, DashboardCaptureDescriptorV1,
    EndpointSelectionCodeV1, InspectionDeliveryCodeV1, InspectionDomainV1, InspectionDropCodeV1,
    InspectionErrorCodeV1, InspectionMeasurementV1, InspectionOperationalStatusV1,
    InspectionQueueSnapshotV1, InspectionSafeFieldsV1, PortSelectionCodeV1,
    ProjectionAvailabilityV1, ProjectionOmissionReasonV1, ProviderProfileCodeV1, RouteCodeV1,
};
use url::{Host, Url};

use crate::codec::{InspectionParsedFactsV1, InspectionStructuralProjectionV1, WireFormat};

#[derive(Clone, Copy)]
struct ProxyInspectionCapturePlanV1 {
    domains: CaptureDomainsV1,
}

impl ProxyInspectionCapturePlanV1 {
    const fn from_descriptor(descriptor: DashboardCaptureDescriptorV1) -> Self {
        Self {
            domains: descriptor.domains(),
        }
    }

    const fn captures(self, domain: InspectionDomainV1) -> bool {
        self.domains.captures(domain)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ProxyInspectionEndpointCodesV1 {
    endpoint: EndpointSelectionCodeV1,
    port: PortSelectionCodeV1,
}

impl ProxyInspectionEndpointCodesV1 {
    pub(crate) fn from_validated_origin(origin: &Url) -> Self {
        let port = if origin.port().is_some() {
            PortSelectionCodeV1::FixedConfigured
        } else {
            PortSelectionCodeV1::NotApplicable
        };
        match (origin.scheme(), origin.host()) {
            ("https", Some(_)) => Self {
                endpoint: EndpointSelectionCodeV1::ConfiguredHttpsOrigin,
                port,
            },
            ("http", Some(Host::Ipv4(address))) if address.is_loopback() => Self {
                endpoint: EndpointSelectionCodeV1::RevalidatedLoopback,
                port,
            },
            ("http", Some(Host::Ipv6(address))) if address.is_loopback() => Self {
                endpoint: EndpointSelectionCodeV1::RevalidatedLoopback,
                port,
            },
            _ => Self {
                endpoint: EndpointSelectionCodeV1::UnavailableFailClosed,
                port: PortSelectionCodeV1::UnavailableFailClosed,
            },
        }
    }
}

/// Opaque installed proxy producer created only by the atomic matched installation operation.
///
/// It exposes no sink, descriptor, registration identity, epoch, lifecycle control, or delivery
/// internals. The activated consumer returned beside it remains the adopter's sole post-install
/// purge/disable authority.
#[must_use]
pub struct ProxyInspectionProducerV1 {
    producer: InstalledInspectionProducerV1,
    capture_plan: ProxyInspectionCapturePlanV1,
}

/// Atomically installs the proxy producer against an adopter-supplied pending consumer half.
///
/// The exact immutable descriptor is used to create the producer pending half. Descriptor
/// mismatch, dispatcher failure, and registration exhaustion remain closed installation errors.
pub fn install_proxy_inspection_v1(
    descriptor: DashboardCaptureDescriptorV1,
    consumer: PendingInspectionConsumerV1,
) -> Result<(ProxyInspectionProducerV1, ActivatedInspectionConsumerV1), InspectionInstallErrorV1> {
    let capture_plan = ProxyInspectionCapturePlanV1::from_descriptor(descriptor);
    let producer = PendingInspectionProducerV1::new(descriptor);
    let (producer, consumer) = install_inspection_v1(producer, consumer)?;
    Ok((
        ProxyInspectionProducerV1 {
            producer,
            capture_plan,
        },
        consumer,
    ))
}

impl ProxyInspectionProducerV1 {
    pub(crate) fn begin_logical(
        &self,
        endpoint_codes: ProxyInspectionEndpointCodesV1,
    ) -> Option<ProxyInspectionLogicalV1> {
        self.producer
            .begin_logical()
            .ok()
            .map(|emitter| ProxyInspectionLogicalV1 {
                emitter,
                capture_plan: self.capture_plan,
                endpoint_codes,
            })
    }
}

pub(crate) struct ProxyInspectionLogicalV1 {
    emitter: InspectionLogicalEmitterV1,
    capture_plan: ProxyInspectionCapturePlanV1,
    endpoint_codes: ProxyInspectionEndpointCodesV1,
}

impl ProxyInspectionLogicalV1 {
    pub(crate) fn emit_request_stages(
        &mut self,
        owner_raw: &[u8],
        provider_visible: &[u8],
        provider_facts: Option<InspectionParsedFactsV1>,
        signed_or_encrypted_surface: bool,
    ) -> [gaze_inspection::InspectionAdmissionOutcomeV1; 2] {
        let owner = self.emitter.try_emit_owner_request(
            accepted_pending_fields(self.endpoint_codes),
            || {
                projection_availability(signed_or_encrypted_surface, || {
                    owner_raw_projection(owner_raw)
                })
            },
        );
        let provider = self.emitter.try_emit_provider_request(
            accepted_pending_fields(self.endpoint_codes),
            move || {
                projection_availability(signed_or_encrypted_surface, || {
                    provider_visible_projection(provider_visible, WireFormat::Json, provider_facts)
                })
            },
        );
        [owner, provider]
    }

    pub(crate) fn emit_response_stages(
        &mut self,
        provider_visible: &[u8],
        owner_restored: &[u8],
        format: WireFormat,
        owner_facts: Option<InspectionParsedFactsV1>,
        signed_or_encrypted_surface: bool,
    ) -> [gaze_inspection::InspectionAdmissionOutcomeV1; 2] {
        let provider = self.emitter.try_emit_provider_response(
            accepted_pending_fields(self.endpoint_codes),
            || {
                projection_availability(signed_or_encrypted_surface, || {
                    provider_visible_projection(provider_visible, format, None)
                })
            },
        );
        let owner = self.emitter.try_emit_owner_restored_response(
            accepted_pending_fields(self.endpoint_codes),
            move || {
                projection_availability(signed_or_encrypted_surface, || {
                    owner_restored_projection(owner_restored, format, owner_facts)
                })
            },
        );
        [provider, owner]
    }

    pub(crate) const fn provider_request_projection_selected(&self) -> bool {
        self.capture_plan
            .captures(InspectionDomainV1::ProviderVisible)
    }

    pub(crate) const fn owner_restored_response_projection_selected(&self) -> bool {
        self.capture_plan
            .captures(InspectionDomainV1::OwnerRestored)
    }
}

// These are admission-time facts: the runtime only materializes the event after accepting it,
// while delivery is necessarily pending until the consumer dequeues that immutable event.
fn accepted_pending_fields(
    endpoint_codes: ProxyInspectionEndpointCodesV1,
) -> InspectionSafeFieldsV1 {
    InspectionSafeFieldsV1::new(
        RouteCodeV1::AnthropicMessages,
        ProviderProfileCodeV1::AnthropicMessagesDirect,
        endpoint_codes.endpoint,
        endpoint_codes.port,
        InspectionOperationalStatusV1::Accepted,
        InspectionErrorCodeV1::None,
        InspectionDropCodeV1::None,
        InspectionDeliveryCodeV1::Pending,
        CoarseDurationBucketV1::NotMeasured,
        InspectionQueueSnapshotV1::default(),
    )
}

fn owner_raw_projection(bytes: &[u8]) -> OwnerRawProjectionV1 {
    let structure = InspectionStructuralProjectionV1::unavailable(WireFormat::Json);
    OwnerRawProjectionV1::new(
        ProjectionAvailabilityV1::Present(OwnerRawPayloadV1::capture(bytes.to_vec())),
        measurement_unavailable(),
        structure.json_shape,
        structure.sse_timeline,
        diagnostic_unavailable(),
        diagnostic_unavailable(),
        diagnostic_unavailable(),
    )
}

fn provider_visible_projection(
    bytes: &[u8],
    format: WireFormat,
    facts: Option<InspectionParsedFactsV1>,
) -> ProviderVisibleProjectionV1 {
    let structure = structural_projection(format, facts);
    ProviderVisibleProjectionV1::new(
        ProjectionAvailabilityV1::Present(ProviderVisiblePayloadV1::capture(bytes.to_vec())),
        measurement_unavailable(),
        structure.json_shape,
        structure.sse_timeline,
        diagnostic_unavailable(),
        diagnostic_unavailable(),
        diagnostic_unavailable(),
    )
}

fn owner_restored_projection(
    bytes: &[u8],
    format: WireFormat,
    facts: Option<InspectionParsedFactsV1>,
) -> OwnerRestoredProjectionV1 {
    let structure = structural_projection(format, facts);
    OwnerRestoredProjectionV1::new(
        ProjectionAvailabilityV1::Present(OwnerRestoredPayloadV1::capture(bytes.to_vec())),
        measurement_unavailable(),
        structure.json_shape,
        structure.sse_timeline,
        diagnostic_unavailable(),
        diagnostic_unavailable(),
        diagnostic_unavailable(),
    )
}

fn measurement_unavailable() -> ProjectionAvailabilityV1<InspectionMeasurementV1> {
    ProjectionAvailabilityV1::Omitted(ProjectionOmissionReasonV1::ProjectionFailedClosed)
}

fn structural_projection(
    format: WireFormat,
    facts: Option<InspectionParsedFactsV1>,
) -> InspectionStructuralProjectionV1 {
    facts.map_or_else(
        || InspectionStructuralProjectionV1::unavailable(format),
        InspectionParsedFactsV1::project,
    )
}

fn diagnostic_unavailable<T>() -> ProjectionAvailabilityV1<T> {
    ProjectionAvailabilityV1::Omitted(ProjectionOmissionReasonV1::ProjectionFailedClosed)
}

fn projection_availability<T>(
    signed_or_encrypted_surface: bool,
    build: impl FnOnce() -> T,
) -> ProjectionAvailabilityV1<T> {
    if signed_or_encrypted_surface {
        ProjectionAvailabilityV1::Omitted(ProjectionOmissionReasonV1::SignedOrEncryptedSurface)
    } else {
        ProjectionAvailabilityV1::Present(build())
    }
}

#[cfg(test)]
mod tests {
    use std::ops::Range;
    use std::sync::{Arc, Condvar, Mutex};
    use std::time::{Duration, Instant};

    use gaze::{Scope, Session};
    use gaze_inspection::{
        InspectionAdmissionOutcomeV1, InspectionEventV1, InspectionQueueLimitsV1, InspectionSink,
        InspectionSinkErrorV1, NoopInspectionSinkV1,
    };
    use gaze_types::inspection::CaptureDomainsV1;

    use super::*;
    use crate::codec::{
        BodyCodec, CodecErrorCode, CodecLimits, RequestPseudonymizer, RequestTransformContext,
        ResponseResidualValidator, ResponseTransformContext,
    };
    use crate::codecs::anthropic::{
        inspection_construction_counts, reset_inspection_construction_counts,
        AnthropicMessagesCodec,
    };

    const SAFE_REQUEST: &[u8] = br#"{"model":"claude-test","max_tokens":32,"messages":[{"role":"user","content":"synthetic text"}]}"#;
    const SAFE_SSE: &[u8] = b"event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"type\":\"message\",\"role\":\"assistant\",\"model\":\"claude-test\",\"content\":[],\"stop_reason\":null,\"stop_sequence\":null,\"usage\":{\"input_tokens\":1,\"output_tokens\":0}}}\n\nevent: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\",\"stop_sequence\":null},\"usage\":{\"output_tokens\":1}}\n\nevent: message_stop\ndata: {\"type\":\"message_stop\"}\n\n";
    const SIGNED_REQUEST: &[u8] = br#"{"model":"claude-test","max_tokens":32,"messages":[{"role":"assistant","content":[{"type":"thinking","thinking":"synthetic reasoning","signature":"opaque-synthetic-signature"}]}]}"#;
    const SIGNED_RESPONSE: &[u8] = br#"{"id":"msg_1","type":"message","role":"assistant","model":"claude-test","content":[{"type":"thinking","thinking":"synthetic reasoning","signature":"opaque-synthetic-signature"}],"stop_reason":"end_turn","stop_sequence":null,"usage":{"input_tokens":1,"output_tokens":1}}"#;
    const SIGNED_SSE: &[u8] = b"event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"type\":\"message\",\"role\":\"assistant\",\"model\":\"claude-test\",\"content\":[],\"stop_reason\":null,\"stop_sequence\":null,\"usage\":{\"input_tokens\":1,\"output_tokens\":0}}}\n\nevent: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"thinking\",\"thinking\":\"\",\"signature\":\"\"}}\n\nevent: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\"synthetic reasoning\"}}\n\nevent: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"signature_delta\",\"signature\":\"opaque-synthetic-signature\"}}\n\nevent: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\nevent: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\",\"stop_sequence\":null},\"usage\":{\"output_tokens\":1}}\n\nevent: message_stop\ndata: {\"type\":\"message_stop\"}\n\n";

    struct IdentityPseudonymizer;

    impl RequestPseudonymizer for IdentityPseudonymizer {
        fn protect(&mut self, input: &str) -> Result<String, CodecErrorCode> {
            Ok(input.to_owned())
        }

        fn protect_with_prefix_cache_write_mode(
            &mut self,
            input: &str,
            _mode: gaze::PrefixCacheWriteMode,
        ) -> Result<String, CodecErrorCode> {
            self.protect(input)
        }

        fn validate_token_shapes(&mut self, _input: &str) -> Result<(), CodecErrorCode> {
            Ok(())
        }
    }

    struct CleanResidual;

    impl ResponseResidualValidator for CleanResidual {
        fn validate(
            &mut self,
            _value: &str,
            _authorized_output_ranges: &[Range<usize>],
        ) -> Result<(), CodecErrorCode> {
            Ok(())
        }
    }

    fn protect_request_for(logical: Option<&ProxyInspectionLogicalV1>) -> crate::ProvedRequestBody {
        let mut pseudonymizer = IdentityPseudonymizer;
        let mut context = RequestTransformContext::new(
            &mut pseudonymizer,
            WireFormat::Json,
            CodecLimits::default(),
        );
        if logical.is_some_and(ProxyInspectionLogicalV1::provider_request_projection_selected) {
            context.request_inspection_projection();
        }
        AnthropicMessagesCodec
            .protect_request(SAFE_REQUEST, &mut context)
            .unwrap()
    }

    fn restore_sse_for(logical: &ProxyInspectionLogicalV1) -> crate::ProvedResponseBody {
        let session = Session::new(Scope::Ephemeral).unwrap();
        let snapshot = session.begin_transaction().commit().unwrap();
        let mut residual = CleanResidual;
        let mut context = ResponseTransformContext::new(
            &snapshot,
            &mut residual,
            WireFormat::Sse,
            CodecLimits::default(),
        );
        if logical.owner_restored_response_projection_selected() {
            context.request_inspection_projection();
        }
        AnthropicMessagesCodec
            .restore_response(SAFE_SSE, &mut context)
            .unwrap()
    }

    #[derive(Default)]
    struct BlockingSink {
        entered: Mutex<usize>,
        entered_changed: Condvar,
        released: Mutex<bool>,
        released_changed: Condvar,
        completed: Mutex<usize>,
        completed_changed: Condvar,
    }

    impl BlockingSink {
        fn wait_until_entered(&self, exact: usize) {
            let guard = self.entered.lock().unwrap();
            let (guard, timeout) = self
                .entered_changed
                .wait_timeout_while(guard, Duration::from_secs(2), |entered| *entered < exact)
                .unwrap();
            assert!(!timeout.timed_out());
            assert_eq!(*guard, exact);
        }

        fn release(&self) {
            *self.released.lock().unwrap() = true;
            self.released_changed.notify_all();
        }

        fn wait_until_completed(&self, exact: usize) {
            let guard = self.completed.lock().unwrap();
            let (guard, timeout) = self
                .completed_changed
                .wait_timeout_while(guard, Duration::from_secs(2), |completed| {
                    *completed < exact
                })
                .unwrap();
            assert!(!timeout.timed_out());
            assert_eq!(*guard, exact);
        }
    }

    impl InspectionSink for BlockingSink {
        fn try_emit(&self, _event: InspectionEventV1) -> Result<(), InspectionSinkErrorV1> {
            *self.entered.lock().unwrap() += 1;
            self.entered_changed.notify_all();
            let guard = self.released.lock().unwrap();
            drop(
                self.released_changed
                    .wait_while(guard, |released| !*released)
                    .unwrap(),
            );
            *self.completed.lock().unwrap() += 1;
            self.completed_changed.notify_all();
            Ok(())
        }
    }

    struct ParkedDispatcher {
        sink: Arc<BlockingSink>,
    }

    impl ParkedDispatcher {
        fn release_and_wait(self, exact_deliveries: usize) {
            self.sink.release();
            self.sink.wait_until_completed(exact_deliveries);
        }
    }

    impl Drop for ParkedDispatcher {
        fn drop(&mut self) {
            self.sink.release();
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
            if let Some(logical) =
                producer.begin_logical(ProxyInspectionEndpointCodesV1::from_validated_origin(
                    &Url::parse("https://api.example.invalid/").unwrap(),
                ))
            {
                return logical;
            }
            assert!(
                Instant::now() < deadline,
                "inspection begin stayed contended"
            );
            std::thread::yield_now();
        }
    }

    fn park_dispatcher_for_test(
        producer: &ProxyInspectionProducerV1,
        sink: Arc<BlockingSink>,
    ) -> ParkedDispatcher {
        let parked = ParkedDispatcher { sink };
        let mut priming = begin_for_test(producer);
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let outcome = priming.emitter.try_emit_owner_request(
                accepted_pending_fields(priming.endpoint_codes),
                || {
                    ProjectionAvailabilityV1::Present(owner_raw_projection(
                        b"synthetic inspection priming",
                    ))
                },
            );
            match outcome {
                InspectionAdmissionOutcomeV1::Accepted { .. } => break,
                InspectionAdmissionOutcomeV1::Dropped(InspectionDropCodeV1::QueueContended) => {
                    assert!(
                        Instant::now() < deadline,
                        "inspection priming stayed contended"
                    );
                    std::thread::yield_now();
                }
                outcome => panic!("inspection priming failed: {outcome:?}"),
            }
        }
        parked.sink.wait_until_entered(1);
        parked
    }

    // `QueueContended` is not a lifecycle verdict: it means the runtime declined to block on its
    // own mutex to read that verdict. A registration's dispatcher thread re-acquires that mutex
    // when `begin_purge`/`disable` notify it, so an emission issued in that window gets the
    // non-answer instead of `Purging`/`Disabled`. Retrying is idempotent — a dropped emission
    // advances no sequence and exhausts no emitter — so ask again rather than weaken the
    // assertion, the same treatment `begin_for_test` and `park_dispatcher_for_test` already give
    // contention.
    fn settled_stages(
        mut emit: impl FnMut() -> [InspectionAdmissionOutcomeV1; 2],
    ) -> [InspectionAdmissionOutcomeV1; 2] {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let outcomes = emit();
            if !outcomes.iter().any(|outcome| {
                matches!(
                    outcome,
                    InspectionAdmissionOutcomeV1::Dropped(InspectionDropCodeV1::QueueContended)
                )
            }) {
                return outcomes;
            }
            assert!(
                Instant::now() < deadline,
                "inspection stages stayed contended: {outcomes:?}"
            );
            std::thread::yield_now();
        }
    }

    #[test]
    fn parked_dispatcher_releases_sink_during_unwind() {
        let sink = Arc::new(BlockingSink::default());
        let (producer, _consumer) = install_for_test(CaptureDomainsV1::All, sink.clone(), 1);
        let unwind = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _parked = park_dispatcher_for_test(&producer, sink.clone());
            panic!("synthetic panic after dispatcher parking");
        }));
        assert!(unwind.is_err());
        sink.wait_until_completed(1);
    }

    #[test]
    fn immutable_capture_plan_requests_only_the_exact_codec_stage() {
        for domains in CaptureDomainsV1::ALL {
            let (producer, _consumer) =
                install_for_test(domains, Arc::new(NoopInspectionSinkV1), 4);
            let logical = begin_for_test(&producer);
            assert_eq!(
                logical.provider_request_projection_selected(),
                domains.captures(gaze_types::inspection::InspectionDomainV1::ProviderVisible)
            );
            assert_eq!(
                logical.owner_restored_response_projection_selected(),
                domains.captures(gaze_types::inspection::InspectionDomainV1::OwnerRestored)
            );
        }
    }

    #[test]
    fn validated_https_and_literal_loopback_http_origins_have_truthful_closed_codes() {
        let https = ProxyInspectionEndpointCodesV1::from_validated_origin(
            &url::Url::parse("https://api.example.invalid/").unwrap(),
        );
        assert_eq!(
            https,
            ProxyInspectionEndpointCodesV1 {
                endpoint: EndpointSelectionCodeV1::ConfiguredHttpsOrigin,
                port: PortSelectionCodeV1::NotApplicable,
            }
        );

        let loopback = ProxyInspectionEndpointCodesV1::from_validated_origin(
            &url::Url::parse("http://127.0.0.1:43123/").unwrap(),
        );
        assert_eq!(
            loopback,
            ProxyInspectionEndpointCodesV1 {
                endpoint: EndpointSelectionCodeV1::RevalidatedLoopback,
                port: PortSelectionCodeV1::FixedConfigured,
            }
        );
    }

    #[test]
    fn every_capture_combination_controls_actual_codec_fact_and_projection_construction() {
        for domains in CaptureDomainsV1::ALL {
            let sink = Arc::new(BlockingSink::default());
            let (producer, _consumer) = install_for_test(domains, sink.clone(), 8);
            let parked = park_dispatcher_for_test(&producer, sink);
            let mut logical = begin_for_test(&producer);
            reset_inspection_construction_counts();
            let mut request = protect_request_for(Some(&logical));
            let request_signed = request.signed_or_encrypted_surface();
            let request_facts = request.take_inspection_parsed_facts();
            assert_eq!(inspection_construction_counts().json_sources_built, 0);
            let request_outcomes = logical.emit_request_stages(
                SAFE_REQUEST,
                request.bytes(),
                request_facts,
                request_signed,
            );
            assert!(request_outcomes
                .iter()
                .all(|outcome| matches!(outcome, InspectionAdmissionOutcomeV1::Accepted { .. })));
            let provider_selected = domains.captures(InspectionDomainV1::ProviderVisible);
            let request_counts = inspection_construction_counts();
            assert_eq!(
                request_counts.json_sources_built,
                usize::from(provider_selected)
            );

            reset_inspection_construction_counts();
            let mut response = restore_sse_for(&logical);
            let response_signed = response.signed_or_encrypted_surface();
            let response_facts = response.take_inspection_parsed_facts();
            let before_response_emit = inspection_construction_counts();
            assert_eq!(before_response_emit.sse_sources_built, 0);
            assert_eq!(before_response_emit.sse_timelines_built, 0);
            let response_outcomes = logical.emit_response_stages(
                SAFE_SSE,
                response.bytes(),
                WireFormat::Sse,
                response_facts,
                response_signed,
            );
            assert!(response_outcomes
                .iter()
                .all(|outcome| matches!(outcome, InspectionAdmissionOutcomeV1::Accepted { .. })));
            let owner_restored_selected = domains.captures(InspectionDomainV1::OwnerRestored);
            let response_counts = inspection_construction_counts();
            assert_eq!(
                response_counts.sse_sources_built,
                usize::from(owner_restored_selected)
            );
            assert_eq!(
                response_counts.sse_timelines_built,
                usize::from(owner_restored_selected)
            );
            parked.release_and_wait(5);
        }
    }

    #[test]
    fn no_dashboard_and_post_begin_lifecycle_fences_build_no_codec_projection() {
        reset_inspection_construction_counts();
        let _request = protect_request_for(None);
        assert_eq!(
            inspection_construction_counts(),
            crate::codecs::anthropic::InspectionConstructionCountsV1::default()
        );

        for disable in [false, true] {
            reset_inspection_construction_counts();
            let (producer, mut consumer) =
                install_for_test(CaptureDomainsV1::All, Arc::new(NoopInspectionSinkV1), 4);
            let mut logical = begin_for_test(&producer);
            let mut request = protect_request_for(Some(&logical));
            let signed = request.signed_or_encrypted_surface();
            let facts = request.take_inspection_parsed_facts();
            if disable {
                consumer.disable();
            } else {
                let _purge = consumer.begin_purge().unwrap();
            }
            let before = inspection_construction_counts();
            logical.emit_request_stages(SAFE_REQUEST, request.bytes(), facts, signed);
            let after = inspection_construction_counts();
            assert_eq!(after.json_sources_built, before.json_sources_built);
        }

        reset_inspection_construction_counts();
        let (producer, _consumer) =
            install_for_test(CaptureDomainsV1::All, Arc::new(NoopInspectionSinkV1), 4);
        let mut logical = begin_for_test(&producer);
        let mut request = protect_request_for(Some(&logical));
        let signed = request.signed_or_encrypted_surface();
        let facts = request.take_inspection_parsed_facts();
        drop(producer);
        let before = inspection_construction_counts();
        logical.emit_request_stages(SAFE_REQUEST, request.bytes(), facts, signed);
        let after = inspection_construction_counts();
        assert_eq!(after.json_sources_built, before.json_sources_built);
    }

    #[test]
    fn signed_request_json_response_and_sse_never_retain_codec_projection_facts() {
        let (producer, _consumer) =
            install_for_test(CaptureDomainsV1::All, Arc::new(NoopInspectionSinkV1), 8);
        let logical = begin_for_test(&producer);

        reset_inspection_construction_counts();
        let mut pseudonymizer = IdentityPseudonymizer;
        let mut request_context = RequestTransformContext::new(
            &mut pseudonymizer,
            WireFormat::Json,
            CodecLimits::default(),
        );
        request_context.request_inspection_projection();
        let mut request = AnthropicMessagesCodec
            .protect_request(SIGNED_REQUEST, &mut request_context)
            .unwrap();
        assert!(request.signed_or_encrypted_surface());
        assert!(request.take_inspection_parsed_facts().is_none());
        assert_eq!(
            inspection_construction_counts(),
            crate::codecs::anthropic::InspectionConstructionCountsV1::default()
        );
        let request_debug = format!("{request:?}");
        assert!(!request_debug.contains("opaque-synthetic-signature"));
        assert!(!request_debug.contains("synthetic reasoning"));

        for (format, input) in [
            (WireFormat::Json, SIGNED_RESPONSE),
            (WireFormat::Sse, SIGNED_SSE),
        ] {
            reset_inspection_construction_counts();
            let session = Session::new(Scope::Ephemeral).unwrap();
            let snapshot = session.begin_transaction().commit().unwrap();
            let mut residual = CleanResidual;
            let mut response_context = ResponseTransformContext::new(
                &snapshot,
                &mut residual,
                format,
                CodecLimits::default(),
            );
            assert!(logical.owner_restored_response_projection_selected());
            response_context.request_inspection_projection();
            let mut response = AnthropicMessagesCodec
                .restore_response(input, &mut response_context)
                .unwrap();
            assert!(response.signed_or_encrypted_surface());
            assert!(response.take_inspection_parsed_facts().is_none());
            assert_eq!(
                inspection_construction_counts(),
                crate::codecs::anthropic::InspectionConstructionCountsV1::default()
            );
            let response_debug = format!("{response:?}");
            assert!(!response_debug.contains("opaque-synthetic-signature"));
            assert!(!response_debug.contains("synthetic reasoning"));
        }
    }

    #[test]
    fn metadata_only_omits_every_content_projection_by_policy() {
        let sink = Arc::new(BlockingSink::default());
        let (producer, _consumer) =
            install_for_test(CaptureDomainsV1::MetadataOnly, sink.clone(), 4);
        let parked = park_dispatcher_for_test(&producer, sink);
        let mut logical = begin_for_test(&producer);
        let outcomes = logical.emit_request_stages(
            b"synthetic owner bytes",
            b"synthetic provider bytes",
            None,
            false,
        );
        assert!(outcomes
            .iter()
            .all(|outcome| matches!(outcome, InspectionAdmissionOutcomeV1::Accepted { .. })));
        parked.release_and_wait(3);
    }

    #[test]
    fn bounded_queue_reports_queue_full_without_blocking_enforcement() {
        let sink = Arc::new(BlockingSink::default());
        let (producer, _consumer) = install_for_test(CaptureDomainsV1::All, sink.clone(), 3);
        let parked = park_dispatcher_for_test(&producer, sink);
        let mut logical = begin_for_test(&producer);
        let request = logical.emit_request_stages(b"owner", b"provider", None, false);
        assert!(request
            .iter()
            .all(|outcome| matches!(outcome, InspectionAdmissionOutcomeV1::Accepted { .. })));
        reset_inspection_construction_counts();
        let mut blocked_request = protect_request_for(Some(&logical));
        let signed = blocked_request.signed_or_encrypted_surface();
        let facts = blocked_request.take_inspection_parsed_facts();
        let response =
            logical.emit_request_stages(SAFE_REQUEST, blocked_request.bytes(), facts, signed);
        assert_eq!(
            response,
            [
                InspectionAdmissionOutcomeV1::Dropped(InspectionDropCodeV1::QueueFull),
                InspectionAdmissionOutcomeV1::Dropped(InspectionDropCodeV1::QueueFull),
            ]
        );
        let counts = inspection_construction_counts();
        assert_eq!(counts.json_sources_built, 1);
        parked.release_and_wait(3);
    }

    #[test]
    fn dropping_proxy_producer_reports_queue_closed_without_building_projection() {
        let (producer, _consumer) =
            install_for_test(CaptureDomainsV1::All, Arc::new(NoopInspectionSinkV1), 4);
        let mut logical = begin_for_test(&producer);
        drop(producer);
        let outcomes = logical.emit_request_stages(b"owner", b"provider", None, false);
        assert_eq!(
            outcomes,
            [
                InspectionAdmissionOutcomeV1::Dropped(InspectionDropCodeV1::QueueClosed),
                InspectionAdmissionOutcomeV1::Dropped(InspectionDropCodeV1::QueueClosed),
            ]
        );
    }

    #[test]
    fn purge_and_disable_report_closed_outcomes_without_building_projection() {
        let (producer, mut consumer) =
            install_for_test(CaptureDomainsV1::All, Arc::new(NoopInspectionSinkV1), 4);
        let mut logical = begin_for_test(&producer);
        let purge = consumer.begin_purge().unwrap();
        assert_eq!(
            settled_stages(|| logical.emit_request_stages(b"owner", b"provider", None, false)),
            [
                InspectionAdmissionOutcomeV1::Dropped(InspectionDropCodeV1::Purging),
                InspectionAdmissionOutcomeV1::Dropped(InspectionDropCodeV1::Purging),
            ]
        );
        purge.complete().unwrap();
        consumer.disable();
        assert_eq!(
            settled_stages(|| logical.emit_response_stages(
                b"provider",
                b"owner",
                WireFormat::Json,
                None,
                false,
            )),
            [
                InspectionAdmissionOutcomeV1::Dropped(InspectionDropCodeV1::Disabled),
                InspectionAdmissionOutcomeV1::Dropped(InspectionDropCodeV1::Disabled),
            ]
        );
    }
}
