use std::sync::Arc;
use std::time::Duration;

use gaze::PrefixCacheWriteMode;
use gaze_proxy::adapters::{GeminiAdapter, OpenAiAdapter};
use gaze_proxy::{
    AdapterContract, AnthropicMessagesCodec, BodyCodec, CodecError, CodecErrorCode, CodecLimits,
    CodecPhase, CoveragePolicy, FormatPolicy, HeaderPolicy, OutputProvenance, PiiSurface,
    ProtocolContract, ProvedRequestBody, ProvedResponseBody, ProviderAdapter, RedirectPolicy,
    RequestPseudonymizer, RequestTransformContext, ResponseResidualValidator,
    ResponseTransformContext, RoutePolicy, SessionPolicy, SessionRegistryConfig,
    SessionRegistryConfigError, SseEvent, WireFormat,
};
use http::Method;
use serde_json::Value;
use url::Url;

struct OldStyleAdapter {
    upstream: Url,
}

#[async_trait::async_trait]
impl ProviderAdapter for OldStyleAdapter {
    fn name(&self) -> &'static str {
        "old-style"
    }

    fn matches_path(&self, method: &Method, path: &str) -> bool {
        method == Method::POST && path == "/v1/legacy"
    }

    fn upstream_base(&self) -> &Url {
        &self.upstream
    }

    fn request_pii_surfaces<'a>(&self, _body: &'a mut Value) -> Vec<PiiSurface<'a>> {
        Vec::new()
    }

    fn response_pii_surfaces<'a>(&self, _body: &'a mut Value) -> Vec<PiiSurface<'a>> {
        Vec::new()
    }

    fn sse_event_pii_surfaces<'a>(&self, _event: &'a mut SseEvent) -> Vec<PiiSurface<'a>> {
        Vec::new()
    }
}

fn assert_legacy(contract: AdapterContract<'_>) {
    assert!(matches!(
        contract.protocol(),
        ProtocolContract::LegacySurfaces
    ));
    assert!(contract.uses_legacy_session_auto_create());
    assert_eq!(contract.session_policy(), None);
    assert_eq!(contract.route_policy(), RoutePolicy::LegacyAdapterMatch);
    assert_eq!(contract.header_policy(), HeaderPolicy::LegacyForwarded);
    assert_eq!(contract.redirect_policy(), RedirectPolicy::LegacyClient);
    assert_eq!(contract.coverage_policy(), CoveragePolicy::LegacySurfaces);
    assert_eq!(contract.formats(), FormatPolicy::legacy());
    assert_eq!(contract.formats().request(), WireFormat::Json);
    assert_eq!(contract.formats().response(), WireFormat::Json);
    assert_eq!(contract.formats().streaming_response(), WireFormat::Sse);
    assert_eq!(contract.codec_limits(), None);
}

#[test]
fn old_style_downstream_adapter_stays_object_safe_and_defaults_legacy() {
    let adapter: Arc<dyn ProviderAdapter> = Arc::new(OldStyleAdapter {
        upstream: Url::parse("https://example.invalid").unwrap(),
    });

    assert_eq!(adapter.name(), "old-style");
    assert!(adapter.matches_path(&Method::POST, "/v1/legacy"));
    assert_legacy(adapter.contract());
}

#[test]
fn openai_and_gemini_remain_explicitly_legacy_compatible() {
    let upstream = Url::parse("https://example.invalid").unwrap();
    let openai = OpenAiAdapter::new(upstream.clone());
    let gemini = GeminiAdapter::new(upstream);

    assert_legacy(openai.contract());
    assert_legacy(gemini.contract());
    assert!(openai.matches_path(&Method::POST, "/v1/chat/completions"));
    assert!(gemini.matches_path(&Method::POST, "/v1beta/models/gemini-test:generateContent"));
}

#[test]
fn accepted_codec_types_are_public_and_protocol_uses_a_borrowed_trait_object() {
    fn accepts_codec(_codec: &dyn BodyCodec) {}

    let codec = AnthropicMessagesCodec;
    accepts_codec(&codec);
    let protocol = ProtocolContract::Codec(&codec);
    assert!(protocol.codec().is_some());
    assert!(!protocol.is_legacy());

    let _: WireFormat = WireFormat::Json;
    let _: CodecLimits = CodecLimits::default();
    let _: CodecPhase = CodecPhase::RequestParse;
    let _: Option<CodecErrorCode> = None;
    let _: Option<CodecError> = None;
    let _: Option<ProvedRequestBody> = None;
    let _: Option<ProvedResponseBody> = None;
    let _: OutputProvenance = OutputProvenance::default();
    let _: Option<RequestTransformContext<'static>> = None;
    let _: Option<ResponseTransformContext<'static>> = None;
    let _: Option<&'static mut dyn RequestPseudonymizer> = None;
    let _: Option<&'static mut dyn ResponseResidualValidator> = None;
}

struct DefaultModePseudonymizer {
    protect_calls: usize,
}

impl RequestPseudonymizer for DefaultModePseudonymizer {
    fn protect(&mut self, input: &str) -> Result<String, CodecErrorCode> {
        self.protect_calls += 1;
        Ok(input.to_owned())
    }

    fn validate_token_shapes(&mut self, _input: &str) -> Result<(), CodecErrorCode> {
        Ok(())
    }
}

#[test]
fn default_request_pseudonymizer_allows_normal_calls_and_fails_closed_on_suppression() {
    let mut pseudonymizer = DefaultModePseudonymizer { protect_calls: 0 };

    assert_eq!(
        pseudonymizer
            .protect_with_prefix_cache_write_mode("synthetic", PrefixCacheWriteMode::Allow)
            .unwrap(),
        "synthetic"
    );
    assert_eq!(pseudonymizer.protect_calls, 1);
    assert_eq!(
        pseudonymizer
            .protect_with_prefix_cache_write_mode("synthetic", PrefixCacheWriteMode::Suppress),
        Err(CodecErrorCode::ProtectionFailedClosed)
    );
    assert_eq!(pseudonymizer.protect_calls, 1);
}

#[test]
fn session_registry_defaults_are_finite_and_invalid_values_fail_closed() {
    let defaults = SessionRegistryConfig::default();
    assert_eq!(defaults.max_active_sessions(), 4_096);
    assert_eq!(defaults.session_ttl(), Duration::from_secs(30 * 60));
    assert_eq!(defaults.max_tombstones(), 4_096);
    assert_eq!(defaults.max_tombstone_bytes(), 1024 * 1024);
    assert_eq!(defaults.tombstone_ttl(), Duration::from_secs(30 * 60));

    assert_eq!(
        SessionRegistryConfig::try_new(
            0,
            defaults.session_ttl(),
            defaults.max_tombstones(),
            defaults.max_tombstone_bytes(),
            defaults.tombstone_ttl(),
        ),
        Err(SessionRegistryConfigError::InvalidMaxActiveSessions)
    );
    assert_eq!(
        SessionRegistryConfig::try_new(
            defaults.max_active_sessions(),
            Duration::ZERO,
            defaults.max_tombstones(),
            defaults.max_tombstone_bytes(),
            defaults.tombstone_ttl(),
        ),
        Err(SessionRegistryConfigError::InvalidSessionTtl)
    );
    assert_eq!(
        SessionRegistryConfig::try_new(
            defaults.max_active_sessions(),
            defaults.session_ttl(),
            0,
            defaults.max_tombstone_bytes(),
            defaults.tombstone_ttl(),
        ),
        Err(SessionRegistryConfigError::InvalidMaxTombstones)
    );
    assert_eq!(
        SessionRegistryConfig::try_new(
            defaults.max_active_sessions(),
            defaults.session_ttl(),
            defaults.max_tombstones(),
            0,
            defaults.tombstone_ttl(),
        ),
        Err(SessionRegistryConfigError::InvalidMaxTombstoneBytes)
    );
    assert_eq!(
        SessionRegistryConfig::try_new(
            defaults.max_active_sessions(),
            defaults.session_ttl(),
            defaults.max_tombstones(),
            defaults.max_tombstone_bytes(),
            Duration::ZERO,
        ),
        Err(SessionRegistryConfigError::InvalidTombstoneTtl)
    );

    assert!(SessionRegistryConfig::try_new(
        SessionRegistryConfig::HARD_MAX_ACTIVE_SESSIONS + 1,
        defaults.session_ttl(),
        defaults.max_tombstones(),
        defaults.max_tombstone_bytes(),
        defaults.tombstone_ttl(),
    )
    .is_err());
    assert!(SessionRegistryConfig::try_new(
        defaults.max_active_sessions(),
        SessionRegistryConfig::HARD_MAX_SESSION_TTL + Duration::from_secs(1),
        defaults.max_tombstones(),
        defaults.max_tombstone_bytes(),
        defaults.tombstone_ttl(),
    )
    .is_err());
    assert!(SessionRegistryConfig::try_new(
        defaults.max_active_sessions(),
        defaults.session_ttl(),
        SessionRegistryConfig::HARD_MAX_TOMBSTONES + 1,
        defaults.max_tombstone_bytes(),
        defaults.tombstone_ttl(),
    )
    .is_err());
    assert!(SessionRegistryConfig::try_new(
        defaults.max_active_sessions(),
        defaults.session_ttl(),
        defaults.max_tombstones(),
        SessionRegistryConfig::HARD_MAX_TOMBSTONE_BYTES + 1,
        defaults.tombstone_ttl(),
    )
    .is_err());
    assert!(SessionRegistryConfig::try_new(
        defaults.max_active_sessions(),
        defaults.session_ttl(),
        defaults.max_tombstones(),
        defaults.max_tombstone_bytes(),
        SessionRegistryConfig::HARD_MAX_TOMBSTONE_TTL + Duration::from_secs(1),
    )
    .is_err());
}

#[test]
fn registry_configuration_can_only_be_lowered_after_construction() {
    let lowered = SessionRegistryConfig::default()
        .try_with_max_active_sessions(128)
        .unwrap()
        .try_with_session_ttl(Duration::from_secs(60))
        .unwrap()
        .try_with_max_tombstones(64)
        .unwrap()
        .try_with_max_tombstone_bytes(64 * 1024)
        .unwrap()
        .try_with_tombstone_ttl(Duration::from_secs(30))
        .unwrap();

    assert_eq!(lowered.max_active_sessions(), 128);
    assert_eq!(lowered.session_ttl(), Duration::from_secs(60));
    assert_eq!(lowered.max_tombstones(), 64);
    assert_eq!(lowered.max_tombstone_bytes(), 64 * 1024);
    assert_eq!(lowered.tombstone_ttl(), Duration::from_secs(30));
    assert_eq!(
        lowered.try_with_max_active_sessions(129),
        Err(SessionRegistryConfigError::NotALowering)
    );
}

#[test]
fn contract_debug_output_is_closed_and_contains_no_sensitive_strings() {
    let codec = AnthropicMessagesCodec;
    let values = format!(
        "{:?} {:?} {:?} {:?}",
        AdapterContract::legacy(),
        ProtocolContract::Codec(&codec),
        SessionPolicy::OpaqueHeaderContinuity(SessionRegistryConfig::default()),
        SessionRegistryConfig::default(),
    );

    for forbidden in [
        "alice@example.invalid",
        "synthetic-secret",
        "https://example.invalid",
        "x-gaze-session-id",
    ] {
        assert!(!values.contains(forbidden));
    }
}
