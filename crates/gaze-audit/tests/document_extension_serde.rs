use gaze_types::{
    CodecAuditRow, CodecCapabilitySet, DocumentExtension, DocumentExtensionBuilder,
    DocumentExtensionError, EmittedTokenSpan, ExtractionDensityPolicy, LeakReportStats, Manifest,
    PiiClass, TextOrigin,
};

fn audit_row() -> CodecAuditRow {
    let mut row = CodecAuditRow::new(
        "gaze.codec.tesseract",
        "gaze-codec-tesseract@0.7.1",
        "image/png",
        TextOrigin::Ocr,
    );
    row.advertised = CodecCapabilitySet::new(true, true, true, false);
    row.delivered = CodecCapabilitySet::new(true, true, false, false);
    row.extraction_density_policy = ExtractionDensityPolicy::Required(1.0);
    row
}

fn extension_builder() -> DocumentExtensionBuilder {
    DocumentExtension::builder(1)
        .clean_md_sha256([1; 32])
        .layout_json_sha256([2; 32])
        .report_json_sha256([3; 32])
        .page_count(2)
        .audit_session_id("018f0000-0000-7000-8000-000000000000")
}

#[test]
fn document_extension_round_trips_with_bundle_root_schema_version() {
    let mut row = audit_row();
    row.options_hash_hex = Some("00".repeat(32));
    row.engine_provenance = Some("tesseract@5.3.4".to_string());
    let extension = extension_builder()
        .preview_png_sha256([4; 32])
        .clean_spans(vec![EmittedTokenSpan::new(0..8, 0..12, PiiClass::Email)])
        .codec_audit(vec![row])
        .build()
        .expect("document extension");

    let json = serde_json::to_value(&extension).expect("serialize document extension");

    assert_eq!(json["schema_version"], 1);
    assert_eq!(json["clean_md_sha256"].as_array().expect("hash").len(), 32);
    assert_eq!(
        json["layout_json_sha256"].as_array().expect("hash").len(),
        32
    );
    assert_eq!(
        json["report_json_sha256"].as_array().expect("hash").len(),
        32
    );
    assert_eq!(
        json["preview_png_sha256"].as_array().expect("hash").len(),
        32
    );
    assert_eq!(json["page_count"], 2);
    assert_eq!(
        json["audit_session_id"],
        "018f0000-0000-7000-8000-000000000000"
    );
    assert_eq!(json["clean_spans"].as_array().expect("spans").len(), 1);
    assert!(json.get("clean_schema_version").is_none());
    assert!(json.get("layout_schema_version").is_none());
    assert!(json.get("report_schema_version").is_none());
    assert!(json.get("manifest_schema_version").is_none());

    let decoded: DocumentExtension =
        serde_json::from_value(json).expect("deserialize document extension");
    assert_eq!(decoded, extension);
}

#[test]
fn document_extension_carries_full_integrity_set() {
    let extension = DocumentExtension::builder(1)
        .clean_md_sha256([10; 32])
        .layout_json_sha256([11; 32])
        .report_json_sha256([12; 32])
        .preview_png_sha256([13; 32])
        .page_count(7)
        .audit_session_id("018f0000-0000-7000-8000-000000000001")
        .clean_spans(vec![EmittedTokenSpan::new(5..14, 20..34, PiiClass::Name)])
        .codec_audit(vec![audit_row()])
        .build()
        .expect("document extension");

    let json = serde_json::to_string(&extension).expect("serialize document extension");
    let decoded: DocumentExtension =
        serde_json::from_str(&json).expect("deserialize document extension");

    assert_eq!(decoded, extension);
    assert_eq!(decoded.clean_md_sha256, [10; 32]);
    assert_eq!(decoded.layout_json_sha256, [11; 32]);
    assert_eq!(decoded.report_json_sha256, [12; 32]);
    assert_eq!(decoded.preview_png_sha256, Some([13; 32]));
    assert_eq!(decoded.page_count, 7);
    assert_eq!(
        decoded.audit_session_id,
        "018f0000-0000-7000-8000-000000000001"
    );
    assert_eq!(decoded.clean_spans.len(), 1);
    assert_eq!(decoded.codec_audit.len(), 1);
}

#[test]
fn document_extension_builder_requires_integrity_fields() {
    assert_eq!(
        DocumentExtension::builder(1).build(),
        Err(DocumentExtensionError::MissingField("clean_md_sha256"))
    );
    assert_eq!(
        DocumentExtension::builder(1)
            .clean_md_sha256([1; 32])
            .layout_json_sha256([2; 32])
            .report_json_sha256([3; 32])
            .page_count(1)
            .build(),
        Err(DocumentExtensionError::MissingField("audit_session_id"))
    );
}

#[test]
fn codec_audit_row_round_trips_without_raw_pii_fields() {
    let row = audit_row();
    let json = serde_json::to_string(&row).expect("serialize codec audit row");

    assert!(json.contains("\"codec_id\""));
    assert!(!json.contains("alice@example.invalid"));
    assert!(!json.contains("\"raw\""));
    assert_eq!(
        serde_json::from_str::<CodecAuditRow>(&json).expect("deserialize codec audit row"),
        row
    );
}

#[test]
fn text_origin_round_trips() {
    for origin in [
        TextOrigin::Ocr,
        TextOrigin::EmbeddedText,
        TextOrigin::Transcript,
        TextOrigin::Hybrid,
    ] {
        let json = serde_json::to_string(&origin).expect("serialize text origin");
        let decoded: TextOrigin = serde_json::from_str(&json).expect("deserialize text origin");
        assert_eq!(decoded, origin);
    }
}

#[test]
fn codec_capability_set_round_trips_and_contains_requested_bits() {
    let delivered = CodecCapabilitySet::new(true, true, false, false);

    let json = serde_json::to_string(&delivered).expect("serialize capabilities");
    let decoded: CodecCapabilitySet =
        serde_json::from_str(&json).expect("deserialize capabilities");

    assert_eq!(decoded, delivered);
    assert!(decoded.contains(CodecCapabilitySet::TEXT_ONLY));
    assert!(!decoded.contains(CodecCapabilitySet::new(true, true, true, false)));
}

#[test]
fn extraction_density_policy_round_trips_closed_variants() {
    for policy in [
        ExtractionDensityPolicy::Required(1.25),
        ExtractionDensityPolicy::Exempt {
            reason: "text_only".to_string(),
        },
    ] {
        let json = serde_json::to_string(&policy).expect("serialize density policy");
        let decoded: ExtractionDensityPolicy =
            serde_json::from_str(&json).expect("deserialize density policy");
        assert_eq!(decoded, policy);
    }
}

#[test]
fn manifest_stats_round_trip_for_document_report_mirrors() {
    let manifest = Manifest::from_spans(vec![EmittedTokenSpan::new(0..15, 0..19, PiiClass::Email)]);
    let mut stats = LeakReportStats::default();
    stats.suspect_count = 1;

    let manifest_json = serde_json::to_string(&manifest).expect("serialize manifest");
    let stats_json = serde_json::to_string(&stats).expect("serialize stats");

    assert_eq!(
        serde_json::from_str::<Manifest>(&manifest_json).expect("deserialize manifest"),
        manifest
    );
    assert_eq!(
        serde_json::from_str::<LeakReportStats>(&stats_json).expect("deserialize stats"),
        stats
    );
}
