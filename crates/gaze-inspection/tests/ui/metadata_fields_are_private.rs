use gaze_types::inspection::*;

fn main() {
    let _meta = InspectionEventMetaV1 {
        version: InspectionMetadataVersionV1::V1,
        logical_id: LogicalInspectionIdV1::new(1),
        emission_id: InspectionEmissionIdV1::new(1),
        sequence: InspectionSequenceV1::new(1),
        epoch: InspectionEpochV1::new(0),
        stage_domain: InspectionStageDomainV1::OwnerRequest,
        route: RouteCodeV1::Unknown,
        profile: ProviderProfileCodeV1::OtherReviewed,
        endpoint: EndpointSelectionCodeV1::UnavailableFailClosed,
        port: PortSelectionCodeV1::UnavailableFailClosed,
        status: InspectionOperationalStatusV1::FailedClosed,
        error: InspectionErrorCodeV1::InternalFailClosed,
        drop: InspectionDropCodeV1::PartialEvent,
        delivery: InspectionDeliveryCodeV1::Dropped,
        duration: CoarseDurationBucketV1::NotMeasured,
        queue: InspectionQueueSnapshotV1::default(),
    };
}
