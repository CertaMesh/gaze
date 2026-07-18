use std::net::{SocketAddr, TcpListener as StdTcpListener};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
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
const PRIMING_ATTEMPT_LIMIT: usize = 8;

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
}

impl RecordingSink {
    fn new(behavior: SinkBehavior) -> Self {
        Self {
            behavior,
            events: Mutex::new(Vec::new()),
            gate: None,
        }
    }

    fn gated(behavior: SinkBehavior, gate: Arc<SinkGate>) -> Self {
        Self {
            behavior,
            events: Mutex::new(Vec::new()),
            gate: Some(gate),
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
            if gate.enter_and_wait(logical_id) {
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

struct SinkGate {
    armed: AtomicBool,
    priming_id: std::sync::OnceLock<LogicalInspectionIdV1>,
    entered: AtomicBool,
    entered_notify: tokio::sync::Notify,
    released: Mutex<bool>,
    released_notify: Condvar,
    released_observed: AtomicBool,
    timed_out: AtomicBool,
}

impl SinkGate {
    fn new() -> Self {
        Self {
            armed: AtomicBool::new(false),
            priming_id: std::sync::OnceLock::new(),
            entered: AtomicBool::new(false),
            entered_notify: tokio::sync::Notify::new(),
            released: Mutex::new(false),
            released_notify: Condvar::new(),
            released_observed: AtomicBool::new(false),
            timed_out: AtomicBool::new(false),
        }
    }

    fn arm(&self) {
        assert!(
            !self.armed.swap(true, Ordering::AcqRel),
            "synthetic inspection priming gate was armed more than once"
        );
        assert!(
            self.priming_id().is_none(),
            "synthetic inspection priming gate claimed a logical ID before arming"
        );
        assert!(
            !self.released_observed.load(Ordering::Acquire),
            "synthetic inspection priming gate was released before arming"
        );
    }

    fn enter_and_wait(&self, logical_id: LogicalInspectionIdV1) -> bool {
        if !self.armed.load(Ordering::Acquire) {
            return false;
        }
        let priming_id = *self.priming_id.get_or_init(|| logical_id);
        if logical_id != priming_id {
            return false;
        }
        if self.entered.swap(true, Ordering::AcqRel) {
            return true;
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
        true
    }

    async fn entered_after_response_drain(&self) -> bool {
        let entered = self.entered_notify.notified();
        if self.entered.load(Ordering::Acquire) {
            return true;
        }
        tokio::select! {
            () = entered => true,
            () = tokio::task::yield_now() => self.entered.load(Ordering::Acquire),
        }
    }

    fn release(&self) {
        let mut released = self
            .released
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *released = true;
        self.released_observed.store(true, Ordering::Release);
        self.released_notify.notify_all();
    }

    fn priming_id(&self) -> Option<LogicalInspectionIdV1> {
        self.priming_id.get().copied()
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
            self.priming_id().is_some(),
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
        self.0.release();
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
        let gate = Arc::new(SinkGate::new());
        let gate_release = SinkGateRelease::new(Arc::clone(&gate));
        let sink = Arc::new(RecordingSink::gated(
            SinkBehavior::Capture,
            Arc::clone(&gate),
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
        for attempt in 1..=PRIMING_ATTEMPT_LIMIT {
            let response = request(&self.proxy, PRIMING_PLACEHOLDER, "/v1/messages").await;
            assert_eq!(
                response.status(),
                StatusCode::OK,
                "synthetic inspection priming request {attempt} failed"
            );
            let _ = response.bytes().await.unwrap();
            if self.gate.entered_after_response_drain().await {
                break;
            }
            assert!(
                attempt < PRIMING_ATTEMPT_LIMIT,
                "synthetic inspection priming gate did not enter after {PRIMING_ATTEMPT_LIMIT} drained attempts; entered={} priming_id_present={} timed_out={}",
                self.gate.entered.load(Ordering::Acquire),
                self.gate.priming_id().is_some(),
                self.gate.timed_out.load(Ordering::Acquire)
            );
        }
        self.gate.assert_parked();
        assert_eq!(
            self.sink.len(),
            0,
            "priming events must not be recorded before the real request"
        );
    }

    async fn release_and_collect(&self, expected_count: usize) -> Vec<RecordedEvent> {
        assert_eq!(
            self.sink.len(),
            0,
            "dispatcher must remain parked in the priming sink callback until explicit release"
        );
        self.gate.assert_parked();
        self.gate.release();
        wait_for_events(&self.sink, expected_count).await;
        self.gate.assert_completed();

        let events = {
            let mut events = self.sink.events.lock().unwrap();
            assert_eq!(events.len(), expected_count);
            std::mem::take(&mut *events)
        };
        let priming_id = self.gate.priming_id().unwrap();
        for event in &events {
            assert_ne!(
                event.logical_id, priming_id,
                "priming logical ID must be filtered from the returned real events"
            );
        }
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
