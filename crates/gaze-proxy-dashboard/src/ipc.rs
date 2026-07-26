// Encoder reachability is blocked on an unforgeable gaze-inspection registration receipt.
#![allow(dead_code)]

use std::io::{self, Read, Write};
use std::net::{Ipv4Addr, SocketAddrV4};
use std::os::unix::net::UnixStream;

use gaze_inspection::{
    InspectionEventV1, InspectionEventViewV1, OwnerRawProjectionV1, OwnerRestoredProjectionV1,
    ProviderVisibleProjectionV1,
};
use gaze_types::inspection::{
    AttestationTraceV1, DecisionTraceV1, InspectionEmissionIdV1, InspectionEpochV1,
    InspectionMeasurementV1, InspectionSequenceV1, InspectionStageDomainV1, JsonShapeSummaryV1,
    LogicalInspectionIdV1, PiiSummaryV1, ProjectionAvailabilityV1, SseTimelineMetaV1,
};
use serde::ser::SerializeStruct as _;
use serde::{Serialize, Serializer};
use zeroize::{Zeroize, Zeroizing};

use crate::auth::PairingSecret;
use crate::{DashboardError, DashboardErrorCode};

const PAIR_MAGIC: &[u8; 4] = b"GZDB";
const IPC_MAGIC: &[u8; 4] = b"GZDI";
const VERSION: u8 = 1;
const IPC_HEADER_LEN: usize = 54;

/// Exact 59-byte child-to-parent pairing frame.
pub struct PairingEnvelopeV1 {
    bytes: Zeroizing<[u8; 59]>,
}

impl PairingEnvelopeV1 {
    /// Encodes the fixed magic/version/nonce/IPv4/port/secret layout.
    #[must_use]
    pub fn encode(nonce: [u8; 16], authority: SocketAddrV4, secret: &PairingSecret) -> Self {
        let mut bytes = Zeroizing::new([0_u8; 59]);
        bytes[0..4].copy_from_slice(PAIR_MAGIC);
        bytes[4] = VERSION;
        bytes[5..21].copy_from_slice(&nonce);
        bytes[21..25].copy_from_slice(&authority.ip().octets());
        bytes[25..27].copy_from_slice(&authority.port().to_be_bytes());
        secret.with_bytes(|secret| bytes[27..59].copy_from_slice(secret));
        Self { bytes }
    }

    /// Parses only the exact canonical frame.
    pub fn decode(mut bytes: [u8; 59]) -> Result<Self, DashboardError> {
        if &bytes[0..4] != PAIR_MAGIC || bytes[4] != VERSION {
            bytes.zeroize();
            return Err(DashboardError::new(DashboardErrorCode::PairingFailed));
        }
        let ip = Ipv4Addr::new(bytes[21], bytes[22], bytes[23], bytes[24]);
        let port = u16::from_be_bytes([bytes[25], bytes[26]]);
        if !ip.is_loopback() || port == 0 {
            bytes.zeroize();
            return Err(DashboardError::new(DashboardErrorCode::PairingFailed));
        }
        Ok(Self {
            bytes: Zeroizing::new(bytes),
        })
    }

    /// Reads exactly one fixed 59-byte frame. Callers own the persistent stream boundary.
    pub fn read_exact(reader: &mut impl Read) -> Result<Self, DashboardError> {
        let mut bytes = [0_u8; 59];
        if reader.read_exact(&mut bytes).is_err() {
            bytes.zeroize();
            return Err(DashboardError::new(DashboardErrorCode::PairingFailed));
        }
        Self::decode(bytes)
    }

    /// Returns the bound literal authority.
    #[must_use]
    pub fn authority(&self) -> SocketAddrV4 {
        SocketAddrV4::new(
            Ipv4Addr::new(
                self.bytes[21],
                self.bytes[22],
                self.bytes[23],
                self.bytes[24],
            ),
            u16::from_be_bytes([self.bytes[25], self.bytes[26]]),
        )
    }

    pub(crate) fn nonce(&self) -> [u8; 16] {
        self.bytes[5..21].try_into().expect("fixed nonce slice")
    }

    pub(crate) fn secret(&self) -> PairingSecret {
        let mut secret = [0_u8; 32];
        secret.copy_from_slice(&self.bytes[27..59]);
        PairingSecret::from_pairing_frame(secret)
    }

    pub(crate) fn write_to(&self, writer: &mut impl Write) -> io::Result<()> {
        writer.write_all(self.bytes.as_ref())
    }
}

/// Exact 22-byte parent-to-child delivery acknowledgement.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct DeliveredAckV1 {
    bytes: [u8; 22],
}

impl DeliveredAckV1 {
    /// Creates the single accepted Delivered status for a nonce.
    #[must_use]
    pub fn delivered(nonce: [u8; 16]) -> Self {
        let mut bytes = [0_u8; 22];
        bytes[0..4].copy_from_slice(PAIR_MAGIC);
        bytes[4] = VERSION;
        bytes[5..21].copy_from_slice(&nonce);
        bytes[21] = 1;
        Self { bytes }
    }

    /// Parses exact magic/version/nonce/status.
    pub fn decode(bytes: [u8; 22], expected_nonce: [u8; 16]) -> Result<Self, DashboardError> {
        if &bytes[0..4] != PAIR_MAGIC
            || bytes[4] != VERSION
            || bytes[5..21] != expected_nonce
            || bytes[21] != 1
        {
            return Err(DashboardError::new(DashboardErrorCode::PairingFailed));
        }
        Ok(Self { bytes })
    }

    pub(crate) fn write_to(&self, writer: &mut impl Write) -> io::Result<()> {
        writer.write_all(&self.bytes)
    }

    pub(crate) fn read_from(
        reader: &mut impl Read,
        expected_nonce: [u8; 16],
    ) -> Result<Self, DashboardError> {
        let mut bytes = [0_u8; 22];
        reader
            .read_exact(&mut bytes)
            .map_err(|_| DashboardError::new(DashboardErrorCode::PairingFailed))?;
        Self::decode(bytes, expected_nonce)
    }
}

pub(crate) fn reject_immediate_trailing(control: &mut UnixStream) -> Result<(), DashboardError> {
    control
        .set_nonblocking(true)
        .map_err(|_| DashboardError::new(DashboardErrorCode::PairingFailed))?;
    let mut trailing = [0_u8; 1];
    let read = control.read(&mut trailing);
    control
        .set_nonblocking(false)
        .map_err(|_| DashboardError::new(DashboardErrorCode::PairingFailed))?;
    match read {
        Err(error) if error.kind() == io::ErrorKind::WouldBlock => Ok(()),
        _ => Err(DashboardError::new(DashboardErrorCode::PairingFailed)),
    }
}

#[derive(Serialize)]
struct SafeProjection<'a> {
    domain_state: BrowserDomainState,
    measurement: BrowserAvailability<'a, InspectionMeasurementV1>,
    json_shape: BrowserAvailability<'a, JsonShapeSummaryV1>,
    sse_timeline: BrowserAvailability<'a, SseTimelineMetaV1>,
    pii_summary: BrowserAvailability<'a, PiiSummaryV1>,
    decision_trace: BrowserAvailability<'a, DecisionTraceV1>,
    attestation: BrowserAvailability<'a, AttestationTraceV1>,
}

#[derive(Serialize)]
#[serde(tag = "kind")]
enum BrowserDomainState {
    Available,
    Omitted { reason: &'static str },
}

struct BrowserAvailability<'a, T>(&'a ProjectionAvailabilityV1<T>);

impl<T> Serialize for BrowserAvailability<'_, T>
where
    T: Serialize,
{
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self.0 {
            ProjectionAvailabilityV1::Present(value) => {
                let mut wire = serializer.serialize_struct("ProjectionAvailability", 2)?;
                wire.serialize_field("state", "Present")?;
                wire.serialize_field("value", value)?;
                wire.end()
            }
            ProjectionAvailabilityV1::Omitted(reason) => {
                let mut wire = serializer.serialize_struct("ProjectionAvailability", 2)?;
                wire.serialize_field("state", "Omitted")?;
                wire.serialize_field("reason", projection_reason_wire(*reason))?;
                wire.end()
            }
        }
    }
}

const fn projection_reason_wire(
    reason: gaze_types::inspection::ProjectionOmissionReasonV1,
) -> &'static str {
    use gaze_types::inspection::ProjectionOmissionReasonV1 as Reason;
    match reason {
        Reason::NotCapturedByPolicy => "NotCapturedByPolicy",
        Reason::NotApplicable => "NotApplicable",
        Reason::UnsupportedFormat => "UnsupportedFormat",
        Reason::MalformedFailClosed => "MalformedFailClosed",
        Reason::LimitExceeded => "LimitExceeded",
        Reason::SignedOrEncryptedSurface => "SignedOrEncryptedSurface",
        Reason::Purging => "Purging",
        Reason::Disabled => "Disabled",
        Reason::QueueClosed => "QueueClosed",
        Reason::ProjectionFailedClosed => "ProjectionFailedClosed",
        Reason::UnknownFuture => "UnknownFuture",
    }
}

/// Zeroizing, globally capped typed IPC frame.
pub(crate) struct EncodedInspectionFrame {
    bytes: Zeroizing<Vec<u8>>,
}

impl EncodedInspectionFrame {
    pub(crate) fn from_event(
        event: &InspectionEventV1,
        max_bytes: usize,
    ) -> Result<Self, DashboardError> {
        let meta = serde_json::to_vec(event.meta())
            .map_err(|_| DashboardError::new(DashboardErrorCode::IpcRejected))?;
        match event.view() {
            InspectionEventViewV1::OwnerRequest(projection) => {
                Self::encode_owner_raw(event, projection, &meta, max_bytes)
            }
            InspectionEventViewV1::ProviderRequest(projection) => {
                Self::encode_provider_visible(event, projection, &meta, max_bytes)
            }
            InspectionEventViewV1::ProviderResponse(projection) => {
                Self::encode_provider_visible(event, projection, &meta, max_bytes)
            }
            InspectionEventViewV1::OwnerRestoredResponse(projection) => {
                Self::encode_owner_restored(event, projection, &meta, max_bytes)
            }
            InspectionEventViewV1::Omitted { reason, .. } => {
                let safe = serialize_omitted_projection(reason)?;
                Self::finish(event, &meta, &safe, &[], max_bytes)
            }
        }
    }

    fn encode_owner_raw(
        event: &InspectionEventV1,
        projection: &OwnerRawProjectionV1,
        meta: &[u8],
        max_bytes: usize,
    ) -> Result<Self, DashboardError> {
        let safe = serialize_projection(projection)?;
        match projection.payload() {
            ProjectionAvailabilityV1::Present(payload) => {
                payload.with_declassified_bytes(|bytes| {
                    Self::finish(event, meta, &safe, bytes, max_bytes)
                })
            }
            ProjectionAvailabilityV1::Omitted(_) => {
                Self::finish(event, meta, &safe, &[], max_bytes)
            }
        }
    }

    fn encode_provider_visible(
        event: &InspectionEventV1,
        projection: &ProviderVisibleProjectionV1,
        meta: &[u8],
        max_bytes: usize,
    ) -> Result<Self, DashboardError> {
        let safe = serialize_projection(projection)?;
        match projection.payload() {
            ProjectionAvailabilityV1::Present(payload) => {
                payload.with_declassified_bytes(|bytes| {
                    Self::finish(event, meta, &safe, bytes, max_bytes)
                })
            }
            ProjectionAvailabilityV1::Omitted(_) => {
                Self::finish(event, meta, &safe, &[], max_bytes)
            }
        }
    }

    fn encode_owner_restored(
        event: &InspectionEventV1,
        projection: &OwnerRestoredProjectionV1,
        meta: &[u8],
        max_bytes: usize,
    ) -> Result<Self, DashboardError> {
        let safe = serialize_projection(projection)?;
        match projection.payload() {
            ProjectionAvailabilityV1::Present(payload) => {
                payload.with_declassified_bytes(|bytes| {
                    Self::finish(event, meta, &safe, bytes, max_bytes)
                })
            }
            ProjectionAvailabilityV1::Omitted(_) => {
                Self::finish(event, meta, &safe, &[], max_bytes)
            }
        }
    }

    fn finish(
        event: &InspectionEventV1,
        meta: &[u8],
        projection: &[u8],
        payload: &[u8],
        max_bytes: usize,
    ) -> Result<Self, DashboardError> {
        let total = IPC_HEADER_LEN
            .checked_add(meta.len())
            .and_then(|value| value.checked_add(projection.len()))
            .and_then(|value| value.checked_add(payload.len()))
            .ok_or_else(|| DashboardError::new(DashboardErrorCode::IpcRejected))?;
        if total > max_bytes || total > u32::MAX as usize {
            return Err(DashboardError::new(DashboardErrorCode::IpcRejected));
        }
        let meta_ref = event.meta();
        let mut bytes = Zeroizing::new(Vec::with_capacity(total));
        bytes.extend_from_slice(IPC_MAGIC);
        bytes.push(VERSION);
        bytes.extend_from_slice(&(total as u32).to_be_bytes());
        bytes.extend_from_slice(&meta_ref.logical_id().get().to_be_bytes());
        bytes.extend_from_slice(&meta_ref.emission_id().get().to_be_bytes());
        bytes.extend_from_slice(&meta_ref.sequence().get().to_be_bytes());
        bytes.extend_from_slice(&meta_ref.epoch().get().to_be_bytes());
        bytes.push(stage_code(meta_ref.stage_domain()));
        bytes.extend_from_slice(&(meta.len() as u32).to_be_bytes());
        bytes.extend_from_slice(&(projection.len() as u32).to_be_bytes());
        bytes.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        bytes.extend_from_slice(meta);
        bytes.extend_from_slice(projection);
        bytes.extend_from_slice(payload);
        debug_assert_eq!(bytes.len(), total);
        Ok(Self { bytes })
    }

    pub(crate) fn as_bytes(&self) -> &[u8] {
        self.bytes.as_ref()
    }
}

fn serialize_projection<T>(projection: &T) -> Result<Vec<u8>, DashboardError>
where
    T: ProjectionAccess,
{
    let safe = SafeProjection {
        domain_state: match projection.payload_omission() {
            None => BrowserDomainState::Available,
            Some(reason) => BrowserDomainState::Omitted {
                reason: projection_reason_wire(reason),
            },
        },
        measurement: BrowserAvailability(projection.measurement()),
        json_shape: BrowserAvailability(projection.json_shape()),
        sse_timeline: BrowserAvailability(projection.sse_timeline()),
        pii_summary: BrowserAvailability(projection.pii_summary()),
        decision_trace: BrowserAvailability(projection.decision_trace()),
        attestation: BrowserAvailability(projection.attestation()),
    };
    serde_json::to_vec(&safe).map_err(|_| DashboardError::new(DashboardErrorCode::IpcRejected))
}

fn serialize_omitted_projection(
    reason: gaze_types::inspection::ProjectionOmissionReasonV1,
) -> Result<Vec<u8>, DashboardError> {
    let reason = projection_reason_wire(reason);
    serde_json::to_vec(&serde_json::json!({
        "domain_state": { "kind": "Omitted", "reason": reason },
        "measurement": { "state": "Omitted", "reason": reason },
        "json_shape": { "state": "Omitted", "reason": reason },
        "sse_timeline": { "state": "Omitted", "reason": reason },
        "pii_summary": { "state": "Omitted", "reason": reason },
        "decision_trace": { "state": "Omitted", "reason": reason },
        "attestation": { "state": "Omitted", "reason": reason }
    }))
    .map_err(|_| DashboardError::new(DashboardErrorCode::IpcRejected))
}

trait ProjectionAccess {
    fn payload_omission(&self) -> Option<gaze_types::inspection::ProjectionOmissionReasonV1>;
    fn measurement(&self) -> &ProjectionAvailabilityV1<InspectionMeasurementV1>;
    fn json_shape(&self) -> &ProjectionAvailabilityV1<JsonShapeSummaryV1>;
    fn sse_timeline(&self) -> &ProjectionAvailabilityV1<SseTimelineMetaV1>;
    fn pii_summary(&self) -> &ProjectionAvailabilityV1<PiiSummaryV1>;
    fn decision_trace(&self) -> &ProjectionAvailabilityV1<DecisionTraceV1>;
    fn attestation(&self) -> &ProjectionAvailabilityV1<AttestationTraceV1>;
}

macro_rules! projection_access {
    ($type:ty) => {
        impl ProjectionAccess for $type {
            fn payload_omission(
                &self,
            ) -> Option<gaze_types::inspection::ProjectionOmissionReasonV1> {
                match self.payload() {
                    ProjectionAvailabilityV1::Present(_) => None,
                    ProjectionAvailabilityV1::Omitted(reason) => Some(*reason),
                }
            }
            fn measurement(&self) -> &ProjectionAvailabilityV1<InspectionMeasurementV1> {
                self.measurement()
            }
            fn json_shape(&self) -> &ProjectionAvailabilityV1<JsonShapeSummaryV1> {
                self.json_shape()
            }
            fn sse_timeline(&self) -> &ProjectionAvailabilityV1<SseTimelineMetaV1> {
                self.sse_timeline()
            }
            fn pii_summary(&self) -> &ProjectionAvailabilityV1<PiiSummaryV1> {
                self.pii_summary()
            }
            fn decision_trace(&self) -> &ProjectionAvailabilityV1<DecisionTraceV1> {
                self.decision_trace()
            }
            fn attestation(&self) -> &ProjectionAvailabilityV1<AttestationTraceV1> {
                self.attestation()
            }
        }
    };
}

projection_access!(OwnerRawProjectionV1);
projection_access!(ProviderVisibleProjectionV1);
projection_access!(OwnerRestoredProjectionV1);

/// Decoded child-owned IPC event.
pub(crate) struct DecodedInspectionFrame {
    pub(crate) logical_id: LogicalInspectionIdV1,
    pub(crate) emission_id: InspectionEmissionIdV1,
    pub(crate) sequence: InspectionSequenceV1,
    pub(crate) epoch: InspectionEpochV1,
    pub(crate) stage_domain: InspectionStageDomainV1,
    pub(crate) safe_meta: Vec<u8>,
    pub(crate) projection: Vec<u8>,
    pub(crate) payload: Zeroizing<Vec<u8>>,
}

impl DecodedInspectionFrame {
    pub(crate) fn decode(bytes: &[u8], max_bytes: usize) -> Result<Self, DashboardError> {
        if bytes.len() < IPC_HEADER_LEN
            || bytes.len() > max_bytes
            || &bytes[0..4] != IPC_MAGIC
            || bytes[4] != VERSION
        {
            return Err(DashboardError::new(DashboardErrorCode::IpcRejected));
        }
        let total = read_u32(bytes, 5)? as usize;
        let meta_len = read_u32(bytes, 42)? as usize;
        let projection_len = read_u32(bytes, 46)? as usize;
        let payload_len = read_u32(bytes, 50)? as usize;
        let expected = IPC_HEADER_LEN
            .checked_add(meta_len)
            .and_then(|value| value.checked_add(projection_len))
            .and_then(|value| value.checked_add(payload_len))
            .ok_or_else(|| DashboardError::new(DashboardErrorCode::IpcRejected))?;
        if total != bytes.len() || expected != total {
            return Err(DashboardError::new(DashboardErrorCode::IpcRejected));
        }
        let meta_end = IPC_HEADER_LEN + meta_len;
        let projection_end = meta_end + projection_len;
        Ok(Self {
            logical_id: LogicalInspectionIdV1::new(read_u64(bytes, 9)?),
            emission_id: InspectionEmissionIdV1::new(read_u64(bytes, 17)?),
            sequence: InspectionSequenceV1::new(read_u64(bytes, 25)?),
            epoch: InspectionEpochV1::new(read_u64(bytes, 33)?),
            stage_domain: decode_stage(bytes[41])?,
            safe_meta: bytes[IPC_HEADER_LEN..meta_end].to_vec(),
            projection: bytes[meta_end..projection_end].to_vec(),
            payload: Zeroizing::new(bytes[projection_end..].to_vec()),
        })
    }

    pub(crate) fn retained_bytes(&self) -> usize {
        IPC_HEADER_LEN + self.safe_meta.len() + self.projection.len() + self.payload.len()
    }

    pub(crate) fn identical_to(&self, other: &Self) -> bool {
        self.logical_id == other.logical_id
            && self.emission_id == other.emission_id
            && self.sequence == other.sequence
            && self.epoch == other.epoch
            && self.stage_domain == other.stage_domain
            && self.safe_meta == other.safe_meta
            && self.projection == other.projection
            && self.payload.as_slice() == other.payload.as_slice()
    }
}

fn stage_code(stage: InspectionStageDomainV1) -> u8 {
    match stage {
        InspectionStageDomainV1::OwnerRequest => 1,
        InspectionStageDomainV1::ProviderRequest => 2,
        InspectionStageDomainV1::ProviderResponse => 3,
        InspectionStageDomainV1::OwnerRestoredResponse => 4,
    }
}

fn decode_stage(code: u8) -> Result<InspectionStageDomainV1, DashboardError> {
    match code {
        1 => Ok(InspectionStageDomainV1::OwnerRequest),
        2 => Ok(InspectionStageDomainV1::ProviderRequest),
        3 => Ok(InspectionStageDomainV1::ProviderResponse),
        4 => Ok(InspectionStageDomainV1::OwnerRestoredResponse),
        _ => Err(DashboardError::new(DashboardErrorCode::IpcRejected)),
    }
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, DashboardError> {
    bytes
        .get(offset..offset + 4)
        .and_then(|slice| slice.try_into().ok())
        .map(u32::from_be_bytes)
        .ok_or_else(|| DashboardError::new(DashboardErrorCode::IpcRejected))
}

fn read_u64(bytes: &[u8], offset: usize) -> Result<u64, DashboardError> {
    bytes
        .get(offset..offset + 8)
        .and_then(|slice| slice.try_into().ok())
        .map(u64::from_be_bytes)
        .ok_or_else(|| DashboardError::new(DashboardErrorCode::IpcRejected))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pairing_frames_are_exact_and_nonce_bound() {
        let secret = PairingSecret::generate().unwrap();
        let nonce = [7_u8; 16];
        let authority = SocketAddrV4::new(Ipv4Addr::new(127, 12, 34, 56), 54321);
        let envelope = PairingEnvelopeV1::encode(nonce, authority, &secret);
        let mut encoded = Vec::new();
        envelope.write_to(&mut encoded).unwrap();
        assert_eq!(encoded.len(), 59);
        let decoded = PairingEnvelopeV1::decode(encoded.try_into().unwrap()).unwrap();
        assert_eq!(decoded.authority(), authority);

        let ack = DeliveredAckV1::delivered(nonce);
        let mut ack_bytes = Vec::new();
        ack.write_to(&mut ack_bytes).unwrap();
        assert_eq!(ack_bytes.len(), 22);
        assert!(DeliveredAckV1::decode(ack_bytes.clone().try_into().unwrap(), nonce).is_ok());
        assert!(DeliveredAckV1::decode(ack_bytes.try_into().unwrap(), [8_u8; 16]).is_err());
    }

    #[test]
    fn persistent_pairing_stream_rejects_immediate_trailing_bytes() {
        let (mut receiver, mut sender) = UnixStream::pair().unwrap();
        assert!(reject_immediate_trailing(&mut receiver).is_ok());
        sender.write_all(&[0x42]).unwrap();
        assert!(reject_immediate_trailing(&mut receiver).is_err());
    }

    #[test]
    fn browser_availability_omits_the_value_container_for_every_closed_reason() {
        use gaze_types::inspection::ProjectionOmissionReasonV1 as Reason;
        for reason in [
            Reason::NotCapturedByPolicy,
            Reason::NotApplicable,
            Reason::UnsupportedFormat,
            Reason::MalformedFailClosed,
            Reason::LimitExceeded,
            Reason::SignedOrEncryptedSurface,
            Reason::Purging,
            Reason::Disabled,
            Reason::QueueClosed,
            Reason::ProjectionFailedClosed,
            Reason::UnknownFuture,
        ] {
            let availability = ProjectionAvailabilityV1::<InspectionMeasurementV1>::Omitted(reason);
            let value = serde_json::to_value(BrowserAvailability(&availability)).unwrap();
            assert_eq!(value["state"], "Omitted");
            assert_eq!(value["reason"], projection_reason_wire(reason));
            assert!(value.get("value").is_none());
        }
    }
}
