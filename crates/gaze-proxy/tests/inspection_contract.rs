use std::net::{SocketAddr, TcpListener as StdTcpListener};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::Duration;

use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use axum::response::IntoResponse;
use axum::routing::post;
use axum::{Json, Router};
use gaze::{Action, ClassRule, DefaultRule, PiiClass, Pipeline};
use gaze_inspection::{
    ActivatedInspectionConsumerV1, InspectionEventKindV1, InspectionEventV1, InspectionEventViewV1,
    InspectionQueueLimitsV1, InspectionSink, InspectionSinkErrorV1, OwnerRawProjectionV1,
    OwnerRestoredProjectionV1, PendingInspectionConsumerV1, ProviderVisibleProjectionV1,
};
use gaze_proxy::adapters::AnthropicAdapter;
use gaze_proxy::inspection::install_proxy_inspection_v1;
use gaze_proxy::{ProxyConfig, ProxyInspectionProducerV1};
use gaze_recognizers::RegexDetector;
use gaze_types::inspection::{
    CaptureDomainsV1, DashboardCaptureDescriptorV1, EndpointSelectionCodeV1,
    InspectionStageDomainV1, LogicalInspectionIdV1, PortSelectionCodeV1, ProjectionAvailabilityV1,
    ProjectionOmissionReasonV1,
};
use reqwest::Client;
use serde_json::{json, Value};
use url::Url;

const SYNTHETIC_EMAIL: &str = "alice@example.invalid";
const PRIMING_PLACEHOLDER: &str = "synthetic inspection priming";
const COMPLETION_PLACEHOLDER: &str = "synthetic inspection completion";

#[derive(Clone, Copy)]
enum SinkBehavior {
    Capture,
    Reject,
    Panic,
}

struct RecordedEvent {
    logical_id: LogicalInspectionIdV1,
    meta: Value,
    kind: InspectionEventKindV1,
    payload: Option<Vec<u8>>,
    availability: Option<RecordedAvailability>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RecordedAvailability {
    measurement_present: bool,
    measurement_omission: Option<ProjectionOmissionReasonV1>,
    json_shape_present: bool,
    json_omission: Option<ProjectionOmissionReasonV1>,
    sse_timeline: Option<Value>,
    sse_omission: Option<ProjectionOmissionReasonV1>,
    pii_omission: ProjectionOmissionReasonV1,
    decision_omission: ProjectionOmissionReasonV1,
    attestation_omission: ProjectionOmissionReasonV1,
}

struct RecordingSink {
    behavior: SinkBehavior,
    events: Mutex<Vec<RecordedEvent>>,
    gate: Option<Arc<SinkGate>>,
    test_control: Option<Arc<SinkTestControl>>,
}

impl RecordingSink {
    fn new(behavior: SinkBehavior) -> Self {
        Self {
            behavior,
            events: Mutex::new(Vec::new()),
            gate: None,
            test_control: None,
        }
    }

    fn gated(
        behavior: SinkBehavior,
        gate: Arc<SinkGate>,
        test_control: Option<Arc<SinkTestControl>>,
    ) -> Self {
        Self {
            behavior,
            events: Mutex::new(Vec::new()),
            gate: Some(gate),
            test_control,
        }
    }

    fn len(&self) -> usize {
        self.events.lock().unwrap().len()
    }

    fn delivery_result(&self) -> Result<(), InspectionSinkErrorV1> {
        match self.behavior {
            SinkBehavior::Capture => Ok(()),
            SinkBehavior::Reject => Err(InspectionSinkErrorV1::Rejected),
            SinkBehavior::Panic => panic!("synthetic inspection sink panic"),
        }
    }
}

impl InspectionSink for RecordingSink {
    fn try_emit(&self, event: InspectionEventV1) -> Result<(), InspectionSinkErrorV1> {
        let logical_id = event.meta().logical_id();
        if let Some(gate) = &self.gate {
            let class = gate.classify(logical_id);
            if let Some(control) = &self.test_control {
                control.before_event(class, event.meta().stage_domain());
            }
            if gate.enter_and_wait(logical_id, event.meta().stage_domain(), class) {
                return self.delivery_result();
            }
        }
        let (payload, availability) = match event.view() {
            InspectionEventViewV1::OwnerRequest(projection) => record_owner_raw(projection),
            InspectionEventViewV1::ProviderRequest(projection)
            | InspectionEventViewV1::ProviderResponse(projection) => {
                record_provider_visible(projection)
            }
            InspectionEventViewV1::OwnerRestoredResponse(projection) => {
                record_owner_restored(projection)
            }
            InspectionEventViewV1::Omitted { .. } => (None, None),
        };
        self.events.lock().unwrap().push(RecordedEvent {
            logical_id,
            meta: serde_json::to_value(event.meta()).unwrap(),
            kind: event.kind(),
            payload,
            availability,
        });
        self.delivery_result()
    }
}

const SINK_GATE_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SinkEventClass {
    Priming,
    Real,
    Completion,
}

struct SinkPause {
    entered: AtomicBool,
    entered_notify: tokio::sync::Notify,
    released: Mutex<bool>,
    released_notify: Condvar,
}

impl SinkPause {
    fn new() -> Self {
        Self {
            entered: AtomicBool::new(false),
            entered_notify: tokio::sync::Notify::new(),
            released: Mutex::new(false),
            released_notify: Condvar::new(),
        }
    }

    fn enter_and_wait(&self) {
        if self.entered.swap(true, Ordering::AcqRel) {
            return;
        }
        self.entered_notify.notify_one();
        let released = self
            .released
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (released, timeout) = self
            .released_notify
            .wait_timeout_while(released, SINK_GATE_TIMEOUT, |released| !*released)
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if timeout.timed_out() && !*released {
            panic!("synthetic inspection test pause timed out after {SINK_GATE_TIMEOUT:?}");
        }
    }

    async fn wait_until_entered(&self) {
        let entered = self.entered_notify.notified();
        if !self.entered.load(Ordering::Acquire) {
            tokio::time::timeout(SINK_GATE_TIMEOUT, entered)
                .await
                .unwrap_or_else(|_| {
                    panic!(
                        "synthetic inspection test pause was not entered after {SINK_GATE_TIMEOUT:?}"
                    )
                });
        }
        assert!(
            self.entered.load(Ordering::Acquire),
            "synthetic inspection test pause notification lacked entered state"
        );
    }

    fn release(&self) {
        let mut released = self
            .released
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *released = true;
        self.released_notify.notify_all();
    }
}

struct SinkPauseRelease(Arc<SinkPause>);

impl SinkPauseRelease {
    fn new(pause: Arc<SinkPause>) -> Self {
        Self(pause)
    }
}

impl Drop for SinkPauseRelease {
    fn drop(&mut self) {
        self.0.release();
    }
}

struct SinkTestControl {
    readiness_pause: Option<Arc<SinkPause>>,
    readiness_pause_used: AtomicBool,
    real_event_pause: Option<(usize, Arc<SinkPause>)>,
    completion_pause: Option<Arc<SinkPause>>,
    real_event_count: AtomicUsize,
}

impl SinkTestControl {
    fn new(
        readiness_pause: Option<Arc<SinkPause>>,
        real_event_pause: Option<(usize, Arc<SinkPause>)>,
        completion_pause: Option<Arc<SinkPause>>,
    ) -> Self {
        Self {
            readiness_pause,
            readiness_pause_used: AtomicBool::new(false),
            real_event_pause,
            completion_pause,
            real_event_count: AtomicUsize::new(0),
        }
    }

    fn before_event(&self, class: SinkEventClass, stage: InspectionStageDomainV1) {
        match class {
            SinkEventClass::Priming => {
                if let Some(pause) = &self.readiness_pause {
                    if !self.readiness_pause_used.swap(true, Ordering::AcqRel) {
                        pause.enter_and_wait();
                    }
                }
            }
            SinkEventClass::Real => {
                let ordinal = self.real_event_count.fetch_add(1, Ordering::AcqRel) + 1;
                if let Some((target, pause)) = &self.real_event_pause {
                    if ordinal == *target {
                        pause.enter_and_wait();
                    }
                }
            }
            SinkEventClass::Completion
                if stage == InspectionStageDomainV1::OwnerRestoredResponse =>
            {
                if let Some(pause) = &self.completion_pause {
                    pause.enter_and_wait();
                }
            }
            SinkEventClass::Completion => {}
        }
    }
}

struct SinkGate {
    armed: AtomicBool,
    priming_request_count: AtomicUsize,
    priming_ids: Mutex<Vec<LogicalInspectionIdV1>>,
    parked_priming_id: OnceLock<LogicalInspectionIdV1>,
    entered: AtomicBool,
    entered_notify: tokio::sync::Notify,
    released: Mutex<bool>,
    released_notify: Condvar,
    released_observed: AtomicBool,
    timed_out: AtomicBool,
    real_request_count: OnceLock<usize>,
    real_ids: Mutex<Vec<LogicalInspectionIdV1>>,
    completion_id: OnceLock<LogicalInspectionIdV1>,
    completion_entered: AtomicBool,
    completion_entered_notify: tokio::sync::Notify,
    completion_released: Mutex<bool>,
    completion_released_notify: Condvar,
    completion_released_observed: AtomicBool,
    completion_timed_out: AtomicBool,
}

impl SinkGate {
    fn new() -> Self {
        Self {
            armed: AtomicBool::new(false),
            priming_request_count: AtomicUsize::new(0),
            priming_ids: Mutex::new(Vec::new()),
            parked_priming_id: OnceLock::new(),
            entered: AtomicBool::new(false),
            entered_notify: tokio::sync::Notify::new(),
            released: Mutex::new(false),
            released_notify: Condvar::new(),
            released_observed: AtomicBool::new(false),
            timed_out: AtomicBool::new(false),
            real_request_count: OnceLock::new(),
            real_ids: Mutex::new(Vec::new()),
            completion_id: OnceLock::new(),
            completion_entered: AtomicBool::new(false),
            completion_entered_notify: tokio::sync::Notify::new(),
            completion_released: Mutex::new(false),
            completion_released_notify: Condvar::new(),
            completion_released_observed: AtomicBool::new(false),
            completion_timed_out: AtomicBool::new(false),
        }
    }

    fn arm(&self) {
        assert!(
            !self.armed.swap(true, Ordering::AcqRel),
            "synthetic inspection priming gate was armed more than once"
        );
        assert!(
            self.priming_ids
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .is_empty(),
            "synthetic inspection priming gate claimed a logical ID before arming"
        );
        assert!(
            !self.released_observed.load(Ordering::Acquire),
            "synthetic inspection priming gate was released before arming"
        );
    }

    fn register_priming_request(&self) {
        assert!(
            self.armed.load(Ordering::Acquire),
            "synthetic inspection priming request was registered before arming"
        );
        assert!(
            self.real_request_count.get().is_none(),
            "synthetic inspection priming request was registered after completion preparation"
        );
        assert!(
            !self.released_observed.load(Ordering::Acquire),
            "synthetic inspection priming request was registered after release"
        );
        self.priming_request_count.fetch_add(1, Ordering::AcqRel);
    }

    fn prepare_completion(&self, real_request_count: usize) {
        assert!(real_request_count > 0);
        self.real_request_count
            .set(real_request_count)
            .unwrap_or_else(|_| {
                panic!("synthetic inspection completion was prepared more than once")
            });
    }

    fn classify(&self, logical_id: LogicalInspectionIdV1) -> SinkEventClass {
        if !self.armed.load(Ordering::Acquire) {
            return SinkEventClass::Real;
        }

        let expected_priming = self.priming_request_count.load(Ordering::Acquire);
        {
            let mut priming_ids = self
                .priming_ids
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if priming_ids.contains(&logical_id) {
                return SinkEventClass::Priming;
            }
            if priming_ids.len() < expected_priming {
                priming_ids.push(logical_id);
                return SinkEventClass::Priming;
            }
        }

        let expected_real = *self.real_request_count.get().unwrap_or_else(|| {
            panic!("real inspection event reached the sink before completion preparation")
        });
        {
            let mut real_ids = self
                .real_ids
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if real_ids.contains(&logical_id) {
                return SinkEventClass::Real;
            }
            if real_ids.len() < expected_real {
                real_ids.push(logical_id);
                return SinkEventClass::Real;
            }
        }

        let completion_id = *self.completion_id.get_or_init(|| logical_id);
        assert_eq!(
            logical_id, completion_id,
            "more than one logical request followed the declared real inspection requests"
        );
        SinkEventClass::Completion
    }

    fn enter_and_wait(
        &self,
        logical_id: LogicalInspectionIdV1,
        stage: InspectionStageDomainV1,
        class: SinkEventClass,
    ) -> bool {
        match class {
            SinkEventClass::Priming => {
                self.enter_priming_and_wait(logical_id);
                true
            }
            SinkEventClass::Real => false,
            SinkEventClass::Completion => {
                if stage == InspectionStageDomainV1::OwnerRestoredResponse {
                    self.enter_completion_and_wait(logical_id);
                }
                true
            }
        }
    }

    fn enter_priming_and_wait(&self, logical_id: LogicalInspectionIdV1) {
        let priming_id = *self.parked_priming_id.get_or_init(|| logical_id);
        if logical_id != priming_id {
            return;
        }
        if self.entered.swap(true, Ordering::AcqRel) {
            return;
        }
        self.entered_notify.notify_one();

        let released = self
            .released
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (released, timeout) = self
            .released_notify
            .wait_timeout_while(released, SINK_GATE_TIMEOUT, |released| !*released)
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if timeout.timed_out() && !*released {
            self.timed_out.store(true, Ordering::Release);
            drop(released);
            panic!(
                "synthetic inspection priming gate timed out after {SINK_GATE_TIMEOUT:?}; entered=true released=false priming_logical_id={}",
                priming_id.get()
            );
        }
    }

    async fn wait_until_priming_parked(&self, deadline: tokio::time::Instant) {
        let entered = self.entered_notify.notified();
        if !self.entered.load(Ordering::Acquire) {
            tokio::time::timeout_at(deadline, entered)
                .await
                .unwrap_or_else(|_| {
                    panic!(
                        "synthetic inspection priming gate did not enter before the {SINK_GATE_TIMEOUT:?} readiness deadline; priming_requests={} priming_ids={} timed_out={}",
                        self.priming_request_count.load(Ordering::Acquire),
                        self.priming_id_count(),
                        self.timed_out.load(Ordering::Acquire)
                    )
                });
        }
        assert!(
            self.entered.load(Ordering::Acquire),
            "synthetic inspection priming notification lacked entered state"
        );
    }

    fn enter_completion_and_wait(&self, logical_id: LogicalInspectionIdV1) {
        assert_eq!(
            self.completion_id.get().copied(),
            Some(logical_id),
            "completion gate was entered by an unclassified logical request"
        );
        if self.completion_entered.swap(true, Ordering::AcqRel) {
            return;
        }
        self.completion_entered_notify.notify_one();

        let released = self
            .completion_released
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (released, timeout) = self
            .completion_released_notify
            .wait_timeout_while(released, SINK_GATE_TIMEOUT, |released| !*released)
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if timeout.timed_out() && !*released {
            self.completion_timed_out.store(true, Ordering::Release);
            drop(released);
            panic!(
                "synthetic inspection completion gate timed out after {SINK_GATE_TIMEOUT:?}; entered=true released=false completion_logical_id={}",
                logical_id.get()
            );
        }
    }

    async fn wait_until_completion_parked(&self, collector_waiting: Option<&tokio::sync::Notify>) {
        let entered = self.completion_entered_notify.notified();
        if !self.completion_entered.load(Ordering::Acquire) {
            if let Some(collector_waiting) = collector_waiting {
                collector_waiting.notify_one();
            }
            tokio::time::timeout(SINK_GATE_TIMEOUT, entered)
                .await
                .unwrap_or_else(|_| {
                    panic!(
                        "synthetic inspection completion gate did not enter after {SINK_GATE_TIMEOUT:?}; real_ids={} completion_id_present={} timed_out={}",
                        self.real_id_count(),
                        self.completion_id.get().is_some(),
                        self.completion_timed_out.load(Ordering::Acquire)
                    )
                });
        }
        assert!(
            self.completion_entered.load(Ordering::Acquire),
            "synthetic inspection completion notification lacked entered state"
        );
    }

    fn release_priming(&self) {
        let mut released = self
            .released
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *released = true;
        self.released_observed.store(true, Ordering::Release);
        self.released_notify.notify_all();
    }

    fn release_completion(&self) {
        let mut released = self
            .completion_released
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *released = true;
        self.completion_released_observed
            .store(true, Ordering::Release);
        self.completion_released_notify.notify_all();
    }

    fn priming_id_count(&self) -> usize {
        self.priming_ids
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .len()
    }

    fn is_priming_id(&self, logical_id: LogicalInspectionIdV1) -> bool {
        self.priming_ids
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .contains(&logical_id)
    }

    fn real_id_count(&self) -> usize {
        self.real_ids
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .len()
    }

    fn assert_parked(&self) {
        assert!(
            self.armed.load(Ordering::Acquire),
            "synthetic inspection priming gate was never armed"
        );
        assert!(
            self.entered.load(Ordering::Acquire),
            "synthetic inspection priming gate was never entered"
        );
        assert!(
            self.parked_priming_id.get().is_some(),
            "synthetic inspection priming gate entered without a logical ID"
        );
        assert!(
            !self.released_observed.load(Ordering::Acquire),
            "synthetic inspection priming gate was released before the real response drained"
        );
        assert!(
            !self.timed_out.load(Ordering::Acquire),
            "synthetic inspection priming gate timed out before explicit release"
        );
    }

    fn assert_completion_parked(&self) {
        assert!(
            self.entered.load(Ordering::Acquire),
            "synthetic inspection priming gate was never entered"
        );
        assert!(
            self.released_observed.load(Ordering::Acquire),
            "synthetic inspection priming gate was not released"
        );
        assert!(
            !self.timed_out.load(Ordering::Acquire),
            "synthetic inspection priming gate timed out before explicit release"
        );
        assert!(
            self.completion_entered.load(Ordering::Acquire),
            "synthetic inspection completion gate was never entered"
        );
        assert!(
            self.completion_id.get().is_some(),
            "synthetic inspection completion gate entered without a logical ID"
        );
        assert_eq!(
            self.priming_id_count(),
            self.priming_request_count.load(Ordering::Acquire),
            "not every registered priming request reached the dispatcher"
        );
        assert_eq!(
            self.real_id_count(),
            *self.real_request_count.get().unwrap(),
            "not every declared real request reached the dispatcher"
        );
        assert!(
            !self.completion_released_observed.load(Ordering::Acquire),
            "synthetic inspection completion gate was released before collection"
        );
        assert!(
            !self.completion_timed_out.load(Ordering::Acquire),
            "synthetic inspection completion gate timed out before collection"
        );
    }

    fn assert_completed(&self) {
        assert!(
            self.entered.load(Ordering::Acquire),
            "synthetic inspection priming gate was never entered"
        );
        assert!(
            self.released_observed.load(Ordering::Acquire),
            "synthetic inspection priming gate was not released"
        );
        assert!(
            !self.timed_out.load(Ordering::Acquire),
            "synthetic inspection priming gate timed out before explicit release"
        );
        assert!(
            self.completion_entered.load(Ordering::Acquire),
            "synthetic inspection completion gate was never entered"
        );
        assert!(
            self.completion_released_observed.load(Ordering::Acquire),
            "synthetic inspection completion gate was not released"
        );
        assert!(
            !self.completion_timed_out.load(Ordering::Acquire),
            "synthetic inspection completion gate timed out before explicit release"
        );
    }
}

struct SinkGateRelease(Arc<SinkGate>);

impl SinkGateRelease {
    fn new(gate: Arc<SinkGate>) -> Self {
        Self(gate)
    }
}

impl Drop for SinkGateRelease {
    fn drop(&mut self) {
        self.0.release_priming();
        self.0.release_completion();
    }
}

fn omission<T>(value: &ProjectionAvailabilityV1<T>) -> ProjectionOmissionReasonV1 {
    match value {
        ProjectionAvailabilityV1::Present(_) => panic!("expected omitted projection"),
        ProjectionAvailabilityV1::Omitted(reason) => *reason,
    }
}

fn record_owner_raw(
    projection: &OwnerRawProjectionV1,
) -> (Option<Vec<u8>>, Option<RecordedAvailability>) {
    let payload = match projection.payload() {
        ProjectionAvailabilityV1::Present(payload) => {
            Some(payload.with_declassified_bytes(<[u8]>::to_vec))
        }
        ProjectionAvailabilityV1::Omitted(_) => None,
    };
    (
        payload,
        Some(record_availability(
            projection.measurement(),
            projection.json_shape(),
            projection.sse_timeline(),
            projection.pii_summary(),
            projection.decision_trace(),
            projection.attestation(),
        )),
    )
}

fn record_provider_visible(
    projection: &ProviderVisibleProjectionV1,
) -> (Option<Vec<u8>>, Option<RecordedAvailability>) {
    let payload = match projection.payload() {
        ProjectionAvailabilityV1::Present(payload) => {
            Some(payload.with_declassified_bytes(<[u8]>::to_vec))
        }
        ProjectionAvailabilityV1::Omitted(_) => None,
    };
    (
        payload,
        Some(record_availability(
            projection.measurement(),
            projection.json_shape(),
            projection.sse_timeline(),
            projection.pii_summary(),
            projection.decision_trace(),
            projection.attestation(),
        )),
    )
}

fn record_owner_restored(
    projection: &OwnerRestoredProjectionV1,
) -> (Option<Vec<u8>>, Option<RecordedAvailability>) {
    let payload = match projection.payload() {
        ProjectionAvailabilityV1::Present(payload) => {
            Some(payload.with_declassified_bytes(<[u8]>::to_vec))
        }
        ProjectionAvailabilityV1::Omitted(_) => None,
    };
    (
        payload,
        Some(record_availability(
            projection.measurement(),
            projection.json_shape(),
            projection.sse_timeline(),
            projection.pii_summary(),
            projection.decision_trace(),
            projection.attestation(),
        )),
    )
}

fn record_availability<M, J, S: serde::Serialize, P, D, A>(
    measurement: &ProjectionAvailabilityV1<M>,
    json_shape: &ProjectionAvailabilityV1<J>,
    sse_timeline: &ProjectionAvailabilityV1<S>,
    pii_summary: &ProjectionAvailabilityV1<P>,
    decision_trace: &ProjectionAvailabilityV1<D>,
    attestation: &ProjectionAvailabilityV1<A>,
) -> RecordedAvailability {
    let json_omission = match json_shape {
        ProjectionAvailabilityV1::Present(_) => None,
        ProjectionAvailabilityV1::Omitted(reason) => Some(*reason),
    };
    let (sse_timeline, sse_omission) = match sse_timeline {
        ProjectionAvailabilityV1::Present(timeline) => {
            (Some(serde_json::to_value(timeline).unwrap()), None)
        }
        ProjectionAvailabilityV1::Omitted(reason) => (None, Some(*reason)),
    };
    RecordedAvailability {
        measurement_present: matches!(measurement, ProjectionAvailabilityV1::Present(_)),
        measurement_omission: match measurement {
            ProjectionAvailabilityV1::Present(_) => None,
            ProjectionAvailabilityV1::Omitted(reason) => Some(*reason),
        },
        json_shape_present: matches!(json_shape, ProjectionAvailabilityV1::Present(_)),
        json_omission,
        sse_timeline,
        sse_omission,
        pii_omission: omission(pii_summary),
        decision_omission: omission(decision_trace),
        attestation_omission: omission(attestation),
    }
}

struct RunningServer {
    base_url: String,
    handle: tokio::task::JoinHandle<()>,
}

impl Drop for RunningServer {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

struct RunningProxy {
    server: RunningServer,
    consumer: Option<ActivatedInspectionConsumerV1>,
}

struct PrimedInspectionHarness {
    _gate_release: SinkGateRelease,
    gate: Arc<SinkGate>,
    sink: Arc<RecordingSink>,
    proxy: RunningProxy,
}

impl PrimedInspectionHarness {
    async fn new(upstream: &RunningServer, domains: CaptureDomainsV1) -> Self {
        let harness = Self::unprimed(upstream, domains).await;
        harness.arm_and_prime().await;
        harness
    }

    async fn unprimed(upstream: &RunningServer, domains: CaptureDomainsV1) -> Self {
        Self::unprimed_with_test_control(upstream, domains, None).await
    }

    async fn unprimed_with_test_control(
        upstream: &RunningServer,
        domains: CaptureDomainsV1,
        test_control: Option<Arc<SinkTestControl>>,
    ) -> Self {
        let gate = Arc::new(SinkGate::new());
        let gate_release = SinkGateRelease::new(Arc::clone(&gate));
        let sink = Arc::new(RecordingSink::gated(
            SinkBehavior::Capture,
            Arc::clone(&gate),
            test_control,
        ));
        let proxy = spawn_proxy(upstream, Some(install(domains, Arc::clone(&sink)))).await;

        Self {
            _gate_release: gate_release,
            gate,
            sink,
            proxy,
        }
    }

    async fn arm_and_prime(&self) {
        assert_eq!(
            self.sink.len(),
            0,
            "inspection priming must start without recorded events"
        );
        self.gate.arm();
        self.gate.register_priming_request();
        let readiness_deadline = tokio::time::Instant::now() + SINK_GATE_TIMEOUT;
        let response = request(&self.proxy, PRIMING_PLACEHOLDER, "/v1/messages").await;
        assert_eq!(
            response.status(),
            StatusCode::OK,
            "synthetic inspection priming request failed"
        );
        let _ = response.bytes().await.unwrap();
        self.gate
            .wait_until_priming_parked(readiness_deadline)
            .await;
        self.gate.assert_parked();
        assert_eq!(
            self.sink.len(),
            0,
            "priming events must not be recorded before the real request"
        );
    }

    async fn enqueue_additional_priming_request(&self) {
        self.gate.assert_parked();
        self.gate.register_priming_request();
        let response = request(&self.proxy, PRIMING_PLACEHOLDER, "/v1/messages").await;
        assert_eq!(
            response.status(),
            StatusCode::OK,
            "additional synthetic inspection priming request failed"
        );
        let _ = response.bytes().await.unwrap();
        assert_eq!(
            self.sink.len(),
            0,
            "additional priming events must remain queued behind the parked dispatcher"
        );
    }

    async fn release_and_collect(&self, expected_count: usize) -> Vec<RecordedEvent> {
        self.release_and_collect_with_fence_ack(expected_count, None)
            .await
    }

    async fn release_and_collect_with_fence_ack(
        &self,
        expected_count: usize,
        collector_waiting: Option<&tokio::sync::Notify>,
    ) -> Vec<RecordedEvent> {
        assert_eq!(
            expected_count % 4,
            0,
            "real inspection event count must contain complete four-stage request groups"
        );
        assert_eq!(
            self.sink.len(),
            0,
            "dispatcher must remain parked in the priming sink callback until explicit release"
        );
        self.gate.assert_parked();
        self.gate.prepare_completion(expected_count / 4);
        let response = request(&self.proxy, COMPLETION_PLACEHOLDER, "/v1/messages").await;
        assert_eq!(
            response.status(),
            StatusCode::OK,
            "synthetic inspection completion request failed"
        );
        let _ = response.bytes().await.unwrap();

        self.gate.release_priming();
        self.gate
            .wait_until_completion_parked(collector_waiting)
            .await;
        self.gate.assert_completion_parked();

        let events = {
            let mut events = self.sink.events.lock().unwrap();
            assert_eq!(
                events.len(),
                expected_count,
                "causally drained inspection snapshot contained a non-real or incomplete event set"
            );
            std::mem::take(&mut *events)
        };
        for event in &events {
            assert!(
                !self.gate.is_priming_id(event.logical_id),
                "priming logical ID must be filtered from the returned real events"
            );
        }
        self.gate.release_completion();
        self.gate.assert_completed();
        events
    }
}

fn assert_request_groups(events: &[RecordedEvent], request_count: usize) {
    assert_eq!(events.len(), request_count * 4);
    let mut logical_ids = Vec::with_capacity(request_count);
    for group in events.chunks_exact(4) {
        let logical_id = group[0].logical_id;
        assert!(
            !logical_ids.contains(&logical_id),
            "real requests must retain distinct logical IDs"
        );
        logical_ids.push(logical_id);
        for (index, event) in group.iter().enumerate() {
            assert_eq!(event.logical_id, logical_id);
            assert_eq!(event.meta["logical_id"], json!(logical_id.get()));
            assert_eq!(event.meta["sequence"], json!((index + 1) as u64));
        }
    }
}

fn assert_four_stage_kinds(events: &[RecordedEvent]) {
    assert_eq!(
        events.iter().map(|event| event.kind).collect::<Vec<_>>(),
        vec![
            InspectionEventKindV1::OwnerRequest,
            InspectionEventKindV1::ProviderRequest,
            InspectionEventKindV1::ProviderResponse,
            InspectionEventKindV1::OwnerRestoredResponse,
        ]
    );
}

fn inspection_pipeline() -> Pipeline {
    Pipeline::builder()
        .detector(RegexDetector::emails().unwrap())
        .rule(ClassRule::new(PiiClass::Email, Action::Tokenize))
        .rule(DefaultRule::new(Action::Preserve))
        .build()
        .unwrap()
}

async fn echo_anthropic_request(request: Request<Body>) -> axum::response::Response {
    let body = to_bytes(request.into_body(), 2 * 1024 * 1024)
        .await
        .unwrap();
    let request_json: Value = serde_json::from_slice(&body).unwrap();
    let protected = request_json["messages"][0]["content"]
        .as_str()
        .unwrap_or("synthetic");
    let signed_response = request_json["model"] == "claude-signed-test";
    if request_json["stream"].as_bool().unwrap_or(false) {
        let frames = if signed_response {
            "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"type\":\"message\",\"role\":\"assistant\",\"model\":\"claude-test\",\"content\":[],\"stop_reason\":null,\"stop_sequence\":null,\"usage\":{\"input_tokens\":1,\"output_tokens\":0}}}\n\nevent: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"thinking\",\"thinking\":\"\",\"signature\":\"\"}}\n\nevent: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\"synthetic reasoning\"}}\n\nevent: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"signature_delta\",\"signature\":\"opaque-synthetic-signature\"}}\n\nevent: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\nevent: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\",\"stop_sequence\":null},\"usage\":{\"output_tokens\":1}}\n\nevent: message_stop\ndata: {\"type\":\"message_stop\"}\n\n".to_owned()
        } else {
            let escaped = serde_json::to_string(protected).unwrap();
            format!(
                "event: message_start\ndata: {{\"type\":\"message_start\",\"message\":{{\"id\":\"msg_1\",\"type\":\"message\",\"role\":\"assistant\",\"model\":\"claude-test\",\"content\":[],\"stop_reason\":null,\"stop_sequence\":null,\"usage\":{{\"input_tokens\":1,\"output_tokens\":0}}}}}}\n\nevent: content_block_start\ndata: {{\"type\":\"content_block_start\",\"index\":0,\"content_block\":{{\"type\":\"text\",\"text\":\"\"}}}}\n\nevent: content_block_delta\ndata: {{\"type\":\"content_block_delta\",\"index\":0,\"delta\":{{\"type\":\"text_delta\",\"text\":{escaped}}}}}\n\nevent: content_block_stop\ndata: {{\"type\":\"content_block_stop\",\"index\":0}}\n\nevent: message_delta\ndata: {{\"type\":\"message_delta\",\"delta\":{{\"stop_reason\":\"end_turn\",\"stop_sequence\":null}},\"usage\":{{\"output_tokens\":1}}}}\n\nevent: message_stop\ndata: {{\"type\":\"message_stop\"}}\n\n"
            )
        };
        return axum::response::Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "text/event-stream")
            .body(Body::from(frames))
            .unwrap();
    }
    if signed_response {
        return Json(json!({
            "id": "msg_1",
            "type": "message",
            "role": "assistant",
            "model": "claude-test",
            "content": [{
                "type": "thinking",
                "thinking": "synthetic reasoning",
                "signature": "opaque-synthetic-signature"
            }],
            "stop_reason": "end_turn",
            "stop_sequence": null,
            "usage": {"input_tokens": 1, "output_tokens": 1}
        }))
        .into_response();
    }
    Json(json!({
        "id": "msg_1",
        "type": "message",
        "role": "assistant",
        "model": "claude-test",
        "content": [{"type": "text", "text": protected}],
        "stop_reason": "end_turn",
        "stop_sequence": null,
        "usage": {"input_tokens": 1, "output_tokens": 1}
    }))
    .into_response()
}

async fn spawn_upstream() -> RunningServer {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let app = Router::new().route("/v1/messages", post(echo_anthropic_request));
    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    RunningServer {
        base_url: format!("http://{address}"),
        handle,
    }
}

fn install(
    domains: CaptureDomainsV1,
    sink: Arc<RecordingSink>,
) -> (ProxyInspectionProducerV1, ActivatedInspectionConsumerV1) {
    let descriptor = DashboardCaptureDescriptorV1::new(domains);
    let limits = InspectionQueueLimitsV1::new(64, 4 * 1024 * 1024).unwrap();
    let consumer = PendingInspectionConsumerV1::new(descriptor, sink, limits);
    install_proxy_inspection_v1(descriptor, consumer).unwrap()
}

async fn spawn_proxy(
    upstream: &RunningServer,
    inspection: Option<(ProxyInspectionProducerV1, ActivatedInspectionConsumerV1)>,
) -> RunningProxy {
    let bind = unused_local_addr();
    let adapter = AnthropicAdapter::new(Url::parse(&upstream.base_url).unwrap());
    let config = ProxyConfig::anthropic_direct(bind, adapter);
    let (config, consumer) = match inspection {
        Some((producer, consumer)) => (config.with_inspection(producer), Some(consumer)),
        None => (config, None),
    };
    let handle = tokio::spawn(async move {
        gaze_proxy::serve(config, Arc::new(inspection_pipeline()))
            .await
            .unwrap();
    });
    wait_for_proxy(bind).await;
    RunningProxy {
        server: RunningServer {
            base_url: format!("http://{bind}"),
            handle,
        },
        consumer,
    }
}

fn unused_local_addr() -> SocketAddr {
    let listener = StdTcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap()
}

async fn wait_for_proxy(bind: SocketAddr) {
    let client = Client::new();
    let health_url = format!("http://{bind}/_gaze_proxy/healthz");
    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    loop {
        if client
            .get(&health_url)
            .send()
            .await
            .is_ok_and(|response| response.status().is_success())
        {
            return;
        }
        assert!(tokio::time::Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

async fn wait_for_events(sink: &RecordingSink, count: usize) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    while sink.len() < count {
        assert!(tokio::time::Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

async fn request(proxy: &RunningProxy, content: &str, path: &str) -> reqwest::Response {
    request_with_stream(proxy, content, path, false).await
}

async fn request_with_stream(
    proxy: &RunningProxy,
    content: &str,
    path: &str,
    stream: bool,
) -> reqwest::Response {
    request_json(
        proxy,
        path,
        json!({
            "model": "claude-test",
            "max_tokens": 32,
            "messages": [{"role": "user", "content": content}],
            "stream": stream
        }),
    )
    .await
}

async fn request_json(proxy: &RunningProxy, path: &str, body: Value) -> reqwest::Response {
    Client::new()
        .post(format!("{}{}", proxy.server.base_url, path))
        .header("x-api-key", "sdk-synthetic-key")
        .header("anthropic-version", "2023-06-01")
        .json(&body)
        .send()
        .await
        .unwrap()
}

fn normalize_meta(mut meta: Value) -> Value {
    let object = meta.as_object_mut().unwrap();
    object.remove("logical_id");
    object.remove("emission_id");
    object.remove("sequence");
    meta
}

#[tokio::test]
async fn delayed_dispatcher_readiness_obeys_the_elapsed_deadline() {
    let upstream = spawn_upstream().await;
    let readiness_pause = Arc::new(SinkPause::new());
    let _readiness_release = SinkPauseRelease::new(Arc::clone(&readiness_pause));
    let control = Arc::new(SinkTestControl::new(
        Some(Arc::clone(&readiness_pause)),
        None,
        None,
    ));
    let harness = PrimedInspectionHarness::unprimed_with_test_control(
        &upstream,
        CaptureDomainsV1::MetadataOnly,
        Some(control),
    )
    .await;

    tokio::join!(harness.arm_and_prime(), async {
        readiness_pause.wait_until_entered().await;
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(
            !harness.gate.entered.load(Ordering::Acquire),
            "readiness pause must precede priming gate entry"
        );
        readiness_pause.release();
    });

    let response = request(&harness.proxy, "synthetic readiness proof", "/v1/messages").await;
    assert_eq!(response.status(), StatusCode::OK);
    let _ = response.bytes().await.unwrap();
    let events = harness.release_and_collect(4).await;
    assert_request_groups(&events, 1);
}

#[tokio::test]
async fn extra_priming_and_delayed_ninth_event_cannot_false_green() {
    let upstream = spawn_upstream().await;
    let late_real_pause = Arc::new(SinkPause::new());
    let completion_pause = Arc::new(SinkPause::new());
    let _late_real_release = SinkPauseRelease::new(Arc::clone(&late_real_pause));
    let _completion_release = SinkPauseRelease::new(Arc::clone(&completion_pause));
    let control = Arc::new(SinkTestControl::new(
        None,
        Some((5, Arc::clone(&late_real_pause))),
        Some(Arc::clone(&completion_pause)),
    ));
    let harness = PrimedInspectionHarness::unprimed_with_test_control(
        &upstream,
        CaptureDomainsV1::MetadataOnly,
        Some(control),
    )
    .await;
    harness.arm_and_prime().await;
    harness.enqueue_additional_priming_request().await;

    let response = request(&harness.proxy, "short synthetic text", "/v1/messages").await;
    assert_eq!(response.status(), StatusCode::OK);
    let _ = response.bytes().await.unwrap();
    let response = request(
        &harness.proxy,
        &format!("longer synthetic text with {SYNTHETIC_EMAIL}"),
        "/v1/messages",
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let _ = response.bytes().await.unwrap();

    let collector_waiting = Arc::new(tokio::sync::Notify::new());
    let collection_returned = Arc::new(tokio::sync::Notify::new());
    let collect = async {
        let events = harness
            .release_and_collect_with_fence_ack(8, Some(&collector_waiting))
            .await;
        collection_returned.notify_one();
        events
    };
    let coordinate = async {
        late_real_pause.wait_until_entered().await;
        assert_eq!(
            harness.gate.priming_id_count(),
            2,
            "every issued priming request must have a classified logical ID"
        );
        assert_eq!(
            harness.sink.len(),
            4,
            "extra priming events must not replace the delayed second real request"
        );
        late_real_pause.release();

        completion_pause.wait_until_entered().await;
        assert_eq!(
            harness.sink.len(),
            8,
            "all real events must precede the completion fence"
        );
        tokio::select! {
            biased;
            _ = collection_returned.notified() => {
                panic!("minimum-count collection returned before the causal completion fence");
            }
            _ = collector_waiting.notified() => {}
        }
        completion_pause.release();
    };
    let (events, ()) = tokio::join!(collect, coordinate);
    assert_request_groups(&events, 2);
}

#[tokio::test]
async fn metadata_only_is_content_independent_and_uses_exact_policy_omissions() {
    let upstream = spawn_upstream().await;
    let harness = PrimedInspectionHarness::new(&upstream, CaptureDomainsV1::MetadataOnly).await;

    let response = request(&harness.proxy, "short synthetic text", "/v1/messages").await;
    assert_eq!(response.status(), StatusCode::OK);
    let _ = response.bytes().await.unwrap();
    let response = request(
        &harness.proxy,
        &format!("longer synthetic text with {SYNTHETIC_EMAIL}"),
        "/v1/messages",
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let _ = response.bytes().await.unwrap();

    let events = harness.release_and_collect(8).await;
    assert_request_groups(&events, 2);
    for event in events.iter() {
        assert_eq!(event.payload, None);
        assert_eq!(event.availability, None);
        assert!(matches!(
            event.kind,
            InspectionEventKindV1::Omitted {
                reason: ProjectionOmissionReasonV1::NotCapturedByPolicy,
                ..
            }
        ));
    }
    let first: Vec<_> = events[0..4]
        .iter()
        .map(|event| normalize_meta(event.meta.clone()))
        .collect();
    let second: Vec<_> = events[4..8]
        .iter()
        .map(|event| normalize_meta(event.meta.clone()))
        .collect();
    assert_eq!(first, second);
    for meta in first {
        let encoded = serde_json::to_string(&meta).unwrap();
        for forbidden in [
            "byte_count",
            "chunk_count",
            "json_shape",
            "sse_timeline",
            "pii_summary",
            "decision_trace",
            "attestation",
            "digest",
            "raw_path",
            SYNTHETIC_EMAIL,
        ] {
            assert!(!encoded.contains(forbidden));
        }
    }
}

#[tokio::test]
async fn all_domains_share_one_logical_id_and_strictly_monotonic_typed_stages() {
    let upstream = spawn_upstream().await;
    let harness = PrimedInspectionHarness::new(&upstream, CaptureDomainsV1::All).await;

    let response = request(&harness.proxy, SYNTHETIC_EMAIL, "/v1/messages").await;
    assert_eq!(response.status(), StatusCode::OK);
    let _ = response.bytes().await.unwrap();
    let events = harness.release_and_collect(4).await;
    assert_request_groups(&events, 1);
    assert_four_stage_kinds(&events);
    let stages = [
        InspectionStageDomainV1::OwnerRequest,
        InspectionStageDomainV1::ProviderRequest,
        InspectionStageDomainV1::ProviderResponse,
        InspectionStageDomainV1::OwnerRestoredResponse,
    ];
    for (index, (event, stage)) in events.iter().zip(stages).enumerate() {
        assert_eq!(
            event.meta["stage_domain"],
            serde_json::to_value(stage).unwrap()
        );
        assert_eq!(event.meta["route"], json!("anthropic_messages"));
        assert_eq!(
            event.meta["endpoint"],
            serde_json::to_value(EndpointSelectionCodeV1::RevalidatedLoopback).unwrap()
        );
        assert_eq!(
            event.meta["port"],
            serde_json::to_value(PortSelectionCodeV1::FixedConfigured).unwrap()
        );
        assert!(event.payload.is_some());
        let json_shape_present = matches!(index, 1 | 3);
        assert_eq!(
            event.availability,
            Some(RecordedAvailability {
                measurement_present: false,
                measurement_omission: Some(ProjectionOmissionReasonV1::ProjectionFailedClosed),
                json_shape_present,
                json_omission: (!json_shape_present)
                    .then_some(ProjectionOmissionReasonV1::ProjectionFailedClosed),
                sse_timeline: None,
                sse_omission: Some(ProjectionOmissionReasonV1::UnsupportedFormat),
                pii_omission: ProjectionOmissionReasonV1::ProjectionFailedClosed,
                decision_omission: ProjectionOmissionReasonV1::ProjectionFailedClosed,
                attestation_omission: ProjectionOmissionReasonV1::ProjectionFailedClosed,
            })
        );
    }
}

#[tokio::test]
async fn sse_response_projects_only_the_closed_ordinal_timeline() {
    let upstream = spawn_upstream().await;
    let harness = PrimedInspectionHarness::new(&upstream, CaptureDomainsV1::All).await;

    let response = request_with_stream(&harness.proxy, SYNTHETIC_EMAIL, "/v1/messages", true).await;
    assert_eq!(response.status(), StatusCode::OK);
    let _ = response.bytes().await.unwrap();
    let events = harness.release_and_collect(4).await;
    assert_request_groups(&events, 1);
    assert_four_stage_kinds(&events);
    for (index, event) in events[..2].iter().enumerate() {
        let availability = event.availability.as_ref().unwrap();
        assert_eq!(availability.json_shape_present, index == 1);
        assert_eq!(
            availability.json_omission,
            (index == 0).then_some(ProjectionOmissionReasonV1::ProjectionFailedClosed)
        );
        assert_eq!(availability.sse_timeline, None);
        assert_eq!(
            availability.sse_omission,
            Some(ProjectionOmissionReasonV1::UnsupportedFormat)
        );
    }
    for (index, event) in events[2..].iter().enumerate() {
        let availability = event.availability.as_ref().unwrap();
        assert!(!availability.json_shape_present);
        assert_eq!(
            availability.json_omission,
            Some(ProjectionOmissionReasonV1::UnsupportedFormat)
        );
        if index == 0 {
            assert_eq!(
                availability.sse_omission,
                Some(ProjectionOmissionReasonV1::ProjectionFailedClosed)
            );
            assert_eq!(availability.sse_timeline, None);
            continue;
        }
        assert_eq!(availability.sse_omission, None);
        let entries = availability.sse_timeline.as_ref().unwrap()["entries"]
            .as_array()
            .unwrap();
        assert_eq!(entries.len(), 6);
        assert_eq!(
            entries
                .iter()
                .map(|entry| entry["ordinal"].as_u64().unwrap())
                .collect::<Vec<_>>(),
            vec![0, 1, 2, 3, 4, 5]
        );
        assert_eq!(entries[2]["event_kind"], json!("content_block_delta"));
        assert_eq!(entries[2]["delta_kind"], json!({"present": "text"}));
        assert_eq!(entries[2]["content_block"], json!({"present": 0}));
    }
}

#[tokio::test]
async fn signed_request_non_stream_and_sse_surfaces_are_omitted_whole() {
    let upstream = spawn_upstream().await;
    let harness = PrimedInspectionHarness::new(&upstream, CaptureDomainsV1::All).await;

    let signed_request = json!({
        "model": "claude-test",
        "max_tokens": 32,
        "messages": [{
            "role": "assistant",
            "content": [{
                "type": "thinking",
                "thinking": "synthetic reasoning",
                "signature": "opaque-synthetic-signature"
            }]
        }]
    });
    let response = request_json(&harness.proxy, "/v1/messages", signed_request).await;
    assert_eq!(response.status(), StatusCode::OK);
    let _ = response.bytes().await.unwrap();

    let signed_response = json!({
        "model": "claude-signed-test",
        "max_tokens": 32,
        "messages": [{"role": "user", "content": "synthetic response request"}],
        "stream": false
    });
    let response = request_json(&harness.proxy, "/v1/messages", signed_response).await;
    assert_eq!(response.status(), StatusCode::OK);
    let _ = response.bytes().await.unwrap();

    let signed_sse = json!({
        "model": "claude-signed-test",
        "max_tokens": 32,
        "messages": [{"role": "user", "content": "synthetic stream request"}],
        "stream": true
    });
    let response = request_json(&harness.proxy, "/v1/messages", signed_sse).await;
    assert_eq!(response.status(), StatusCode::OK);
    let _ = response.bytes().await.unwrap();

    let events = harness.release_and_collect(12).await;
    assert_request_groups(&events, 3);
    for event in [
        &events[0],
        &events[1],
        &events[6],
        &events[7],
        &events[10],
        &events[11],
    ] {
        assert_eq!(event.payload, None);
        assert_eq!(event.availability, None);
        assert!(matches!(
            event.kind,
            InspectionEventKindV1::Omitted {
                reason: ProjectionOmissionReasonV1::SignedOrEncryptedSurface,
                ..
            }
        ));
        let encoded = serde_json::to_string(&event.meta).unwrap();
        assert!(!encoded.contains("opaque-synthetic-signature"));
        assert!(!encoded.contains("synthetic reasoning"));
    }
}

#[tokio::test]
async fn panicking_sink_never_changes_proxy_enforcement() {
    let upstream = spawn_upstream().await;
    let sink = Arc::new(RecordingSink::new(SinkBehavior::Panic));
    let proxy = spawn_proxy(
        &upstream,
        Some(install(CaptureDomainsV1::MetadataOnly, Arc::clone(&sink))),
    )
    .await;

    assert_eq!(
        request(&proxy, SYNTHETIC_EMAIL, "/v1/messages")
            .await
            .status(),
        StatusCode::OK
    );
    wait_for_events(&sink, 4).await;
}

#[tokio::test]
async fn purge_disable_and_hostile_sink_never_change_proxy_enforcement() {
    let upstream = spawn_upstream().await;
    let sink = Arc::new(RecordingSink::new(SinkBehavior::Reject));
    let mut proxy = spawn_proxy(
        &upstream,
        Some(install(CaptureDomainsV1::MetadataOnly, Arc::clone(&sink))),
    )
    .await;

    let purge = proxy.consumer.as_mut().unwrap().begin_purge().unwrap();
    assert_eq!(
        request(&proxy, SYNTHETIC_EMAIL, "/v1/messages")
            .await
            .status(),
        StatusCode::OK
    );
    assert_eq!(sink.len(), 0);
    purge.complete().unwrap();
    assert_eq!(
        request(&proxy, SYNTHETIC_EMAIL, "/v1/messages")
            .await
            .status(),
        StatusCode::OK
    );
    wait_for_events(&sink, 4).await;
    proxy.consumer.as_mut().unwrap().disable();
    assert_eq!(
        request(&proxy, SYNTHETIC_EMAIL, "/v1/messages")
            .await
            .status(),
        StatusCode::OK
    );
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(sink.len(), 4);
}

#[tokio::test]
async fn route_code_is_stamped_only_after_exact_direct_route_validation() {
    let upstream = spawn_upstream().await;
    let harness =
        PrimedInspectionHarness::unprimed(&upstream, CaptureDomainsV1::MetadataOnly).await;

    let response = request(&harness.proxy, SYNTHETIC_EMAIL, "/v1/messages/").await;
    assert_ne!(response.status(), StatusCode::OK);
    let _ = response.bytes().await.unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(harness.sink.len(), 0);

    harness.arm_and_prime().await;
    let response = request(&harness.proxy, SYNTHETIC_EMAIL, "/v1/messages").await;
    assert_eq!(response.status(), StatusCode::OK);
    let _ = response.bytes().await.unwrap();
    let events = harness.release_and_collect(4).await;
    assert_request_groups(&events, 1);
    for event in &events {
        assert_eq!(event.meta["route"], json!("anthropic_messages"));
    }
}

#[tokio::test]
async fn no_dashboard_configuration_keeps_the_proxy_path_independent() {
    let upstream = spawn_upstream().await;
    let proxy = spawn_proxy(&upstream, None).await;
    assert!(proxy.consumer.is_none());
    assert_eq!(
        request(&proxy, SYNTHETIC_EMAIL, "/v1/messages")
            .await
            .status(),
        StatusCode::OK
    );
}
