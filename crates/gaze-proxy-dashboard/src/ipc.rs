// Encoder reachability is blocked on an unforgeable gaze-inspection registration receipt.
#![allow(dead_code)]

use std::io::{self, Read, Write};
use std::net::{Ipv4Addr, SocketAddrV4};
use std::os::unix::net::UnixStream;

use gaze_inspection::{InspectionEventV1, InspectionEventViewV1};
use gaze_types::inspection::{
    DashboardCaptureDescriptorV1, InspectionEmissionIdV1, InspectionEpochV1, InspectionSequenceV1,
    InspectionStageDomainV1, LogicalInspectionIdV1, ProjectionAvailabilityV1,
};
use zeroize::{Zeroize, Zeroizing};

use crate::auth::PairingSecret;
use crate::snapshot::{CaptureTierV1, MetaModelV1, PayloadAvailabilityV1, StageModelV1};
use crate::{DashboardError, DashboardErrorCode};

const PAIR_MAGIC: &[u8; 4] = b"GZDB";
const IPC_MAGIC: &[u8; 4] = b"GZDI";
const STATUS_MAGIC: &[u8; 4] = b"GZDS";
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

// Runtime pairing is deliberately incompatible with V1 peers: neither side may
// activate until the child has accepted delivery and acknowledged completion.
const PAIR_VERSION_V2: u8 = 2;
const ENVELOPE_V2: u8 = 1;
const DELIVERED_V2: u8 = 2;
const READY_V2: u8 = 3;

pub(crate) struct PairingEnvelopeV2 {
    bytes: Zeroizing<[u8; 60]>,
}

impl PairingEnvelopeV2 {
    pub(crate) fn encode(nonce: [u8; 16], authority: SocketAddrV4, secret: &PairingSecret) -> Self {
        let mut bytes = Zeroizing::new([0; 60]);
        bytes[..4].copy_from_slice(PAIR_MAGIC);
        bytes[4] = PAIR_VERSION_V2;
        bytes[5] = ENVELOPE_V2;
        bytes[6..22].copy_from_slice(&nonce);
        bytes[22..26].copy_from_slice(&authority.ip().octets());
        bytes[26..28].copy_from_slice(&authority.port().to_be_bytes());
        secret.with_bytes(|secret| bytes[28..].copy_from_slice(secret));
        Self { bytes }
    }

    pub(crate) fn read_exact(reader: &mut impl Read) -> Result<Self, DashboardError> {
        let mut bytes = Zeroizing::new([0; 60]);
        reader
            .read_exact(&mut bytes[..6])
            .map_err(|_| pairing_failed())?;
        if &bytes[..4] != PAIR_MAGIC || bytes[4] != PAIR_VERSION_V2 || bytes[5] != ENVELOPE_V2 {
            return Err(pairing_failed());
        }
        reader
            .read_exact(&mut bytes[6..])
            .map_err(|_| pairing_failed())?;
        let envelope = Self { bytes };
        let authority = envelope.authority();
        if !authority.ip().is_loopback() || authority.port() == 0 {
            return Err(pairing_failed());
        }
        Ok(envelope)
    }

    pub(crate) fn authority(&self) -> SocketAddrV4 {
        SocketAddrV4::new(
            Ipv4Addr::new(
                self.bytes[22],
                self.bytes[23],
                self.bytes[24],
                self.bytes[25],
            ),
            u16::from_be_bytes([self.bytes[26], self.bytes[27]]),
        )
    }

    pub(crate) fn nonce(&self) -> [u8; 16] {
        self.bytes[6..22].try_into().expect("fixed nonce slice")
    }

    pub(crate) fn secret(&self) -> PairingSecret {
        PairingSecret::from_pairing_frame(
            self.bytes[28..60].try_into().expect("fixed secret slice"),
        )
    }

    pub(crate) fn write_to(&self, writer: &mut impl Write) -> io::Result<()> {
        writer.write_all(self.bytes.as_ref())
    }
}

pub(crate) struct DeliveredAckV2;
pub(crate) struct PairingReadyV2;

impl DeliveredAckV2 {
    pub(crate) fn write_to(writer: &mut impl Write, nonce: [u8; 16]) -> io::Result<()> {
        writer.write_all(&pairing_status_frame(DELIVERED_V2, nonce))
    }

    pub(crate) fn read_from(reader: &mut impl Read, nonce: [u8; 16]) -> Result<(), DashboardError> {
        read_pairing_status(reader, DELIVERED_V2, nonce)
    }
}

impl PairingReadyV2 {
    pub(crate) fn write_to(writer: &mut impl Write, nonce: [u8; 16]) -> io::Result<()> {
        writer.write_all(&pairing_status_frame(READY_V2, nonce))
    }

    pub(crate) fn read_from(reader: &mut impl Read, nonce: [u8; 16]) -> Result<(), DashboardError> {
        read_pairing_status(reader, READY_V2, nonce)
    }
}

// Delivered and Ready share nonce/status validation, but have distinct wire kinds.
fn pairing_status_frame(kind: u8, nonce: [u8; 16]) -> [u8; 23] {
    let mut bytes = [0; 23];
    bytes[..4].copy_from_slice(PAIR_MAGIC);
    bytes[4] = PAIR_VERSION_V2;
    bytes[5] = kind;
    bytes[6..22].copy_from_slice(&nonce);
    bytes[22] = 1;
    bytes
}

fn read_pairing_status(
    reader: &mut impl Read,
    kind: u8,
    nonce: [u8; 16],
) -> Result<(), DashboardError> {
    let mut bytes = [0; 23];
    reader
        .read_exact(&mut bytes)
        .map_err(|_| pairing_failed())?;
    if bytes != pairing_status_frame(kind, nonce) {
        return Err(pairing_failed());
    }
    Ok(())
}

fn pairing_failed() -> DashboardError {
    DashboardError::new(DashboardErrorCode::PairingFailed)
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

/// Zeroizing, globally capped typed IPC frame.
pub(crate) struct EncodedInspectionFrame {
    bytes: Zeroizing<Vec<u8>>,
}

macro_rules! encode_domain_projection {
    ($event:expr, $meta:expr, $projection:expr, $model:expr, $max_bytes:expr) => {{
        let safe = serialize_stage_model(&$model)?;
        match $projection.payload() {
            ProjectionAvailabilityV1::Present(payload) => {
                payload.with_declassified_bytes(|bytes| {
                    Self::finish($event, $meta, &safe, bytes, $max_bytes)
                })
            }
            ProjectionAvailabilityV1::Omitted(_) => {
                Self::finish($event, $meta, &safe, &[], $max_bytes)
            }
        }
    }};
}

impl EncodedInspectionFrame {
    pub(crate) fn from_event(
        event: &InspectionEventV1,
        max_bytes: usize,
    ) -> Result<Self, DashboardError> {
        let meta = serde_json::to_vec(event.meta())
            .map_err(|_| DashboardError::new(DashboardErrorCode::IpcRejected))?;
        match event.view() {
            InspectionEventViewV1::OwnerRequest(projection) => encode_domain_projection!(
                event,
                &meta,
                projection,
                StageModelV1::from_owner_raw(projection),
                max_bytes
            ),
            InspectionEventViewV1::ProviderRequest(projection)
            | InspectionEventViewV1::ProviderResponse(projection) => encode_domain_projection!(
                event,
                &meta,
                projection,
                StageModelV1::from_provider_visible(projection),
                max_bytes
            ),
            InspectionEventViewV1::OwnerRestoredResponse(projection) => {
                encode_domain_projection!(
                    event,
                    &meta,
                    projection,
                    StageModelV1::from_owner_restored(projection),
                    max_bytes
                )
            }
            InspectionEventViewV1::Omitted { reason, .. } => {
                let safe = serialize_stage_model(&StageModelV1::omitted(reason))?;
                Self::finish(event, &meta, &safe, &[], max_bytes)
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

fn serialize_stage_model(model: &StageModelV1) -> Result<Vec<u8>, DashboardError> {
    serde_json::to_vec(model).map_err(|_| DashboardError::new(DashboardErrorCode::IpcRejected))
}

/// Exact 22-byte parent-to-child runtime status frame: the immutable startup capture
/// descriptor plus saturating parent ingress drop counters.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct RuntimeStatusFrameV1 {
    pub(crate) capture: CaptureTierV1,
    pub(crate) ingress_full: u64,
    pub(crate) ingress_closed: u64,
}

impl RuntimeStatusFrameV1 {
    pub(crate) const LEN: usize = 22;

    /// Encodes one status frame. Only the four dashboard acceptance descriptors exist.
    pub(crate) fn encode(
        descriptor: DashboardCaptureDescriptorV1,
        ingress_full: u64,
        ingress_closed: u64,
    ) -> Result<[u8; Self::LEN], DashboardError> {
        let code = match CaptureTierV1::from_domains(descriptor.domains()) {
            Some(CaptureTierV1::ProviderVisible) => 1,
            Some(CaptureTierV1::OwnerRaw) => 2,
            Some(CaptureTierV1::OwnerRestored) => 3,
            Some(CaptureTierV1::Both) => 4,
            Some(CaptureTierV1::NotReported) | None => {
                return Err(DashboardError::new(DashboardErrorCode::IpcRejected))
            }
        };
        let mut bytes = [0_u8; Self::LEN];
        bytes[..4].copy_from_slice(STATUS_MAGIC);
        bytes[4] = VERSION;
        bytes[5] = code;
        bytes[6..14].copy_from_slice(&ingress_full.to_be_bytes());
        bytes[14..22].copy_from_slice(&ingress_closed.to_be_bytes());
        Ok(bytes)
    }

    fn decode(bytes: &[u8]) -> Result<Self, DashboardError> {
        if bytes.len() != Self::LEN || &bytes[..4] != STATUS_MAGIC || bytes[4] != VERSION {
            return Err(DashboardError::new(DashboardErrorCode::IpcRejected));
        }
        let capture = match bytes[5] {
            1 => CaptureTierV1::ProviderVisible,
            2 => CaptureTierV1::OwnerRaw,
            3 => CaptureTierV1::OwnerRestored,
            4 => CaptureTierV1::Both,
            _ => return Err(DashboardError::new(DashboardErrorCode::IpcRejected)),
        };
        Ok(Self {
            capture,
            ingress_full: read_u64(bytes, 6)?,
            ingress_closed: read_u64(bytes, 14)?,
        })
    }
}

/// One decoded inbound frame on the child inspection channel.
pub(crate) enum InboundFrameV1 {
    Event(Box<DecodedInspectionFrame>),
    Status(RuntimeStatusFrameV1),
}

impl InboundFrameV1 {
    pub(crate) fn decode(bytes: &[u8], max_bytes: usize) -> Result<Self, DashboardError> {
        if bytes.starts_with(STATUS_MAGIC) {
            RuntimeStatusFrameV1::decode(bytes).map(Self::Status)
        } else {
            DecodedInspectionFrame::decode(bytes, max_bytes)
                .map(|frame| Self::Event(Box::new(frame)))
        }
    }
}

/// Decoded child-owned IPC event. Metadata and projection are typed; payload availability
/// is the projection's typed field, never the payload length.
pub(crate) struct DecodedInspectionFrame {
    pub(crate) logical_id: LogicalInspectionIdV1,
    pub(crate) emission_id: InspectionEmissionIdV1,
    pub(crate) sequence: InspectionSequenceV1,
    pub(crate) epoch: InspectionEpochV1,
    pub(crate) stage_domain: InspectionStageDomainV1,
    pub(crate) meta: MetaModelV1,
    pub(crate) projection: StageModelV1,
    pub(crate) safe_wire_bytes: usize,
    pub(crate) payload: Zeroizing<Vec<u8>>,
}

impl DecodedInspectionFrame {
    pub(crate) fn decode(bytes: &[u8], max_bytes: usize) -> Result<Self, DashboardError> {
        let rejected = || DashboardError::new(DashboardErrorCode::IpcRejected);
        if bytes.len() < IPC_HEADER_LEN
            || bytes.len() > max_bytes
            || &bytes[0..4] != IPC_MAGIC
            || bytes[4] != VERSION
        {
            return Err(rejected());
        }
        let total = read_u32(bytes, 5)? as usize;
        let meta_len = read_u32(bytes, 42)? as usize;
        let projection_len = read_u32(bytes, 46)? as usize;
        let payload_len = read_u32(bytes, 50)? as usize;
        let expected = IPC_HEADER_LEN
            .checked_add(meta_len)
            .and_then(|value| value.checked_add(projection_len))
            .and_then(|value| value.checked_add(payload_len))
            .ok_or_else(rejected)?;
        if total != bytes.len() || expected != total {
            return Err(rejected());
        }
        let meta_end = IPC_HEADER_LEN + meta_len;
        let projection_end = meta_end + projection_len;
        let meta: MetaModelV1 =
            serde_json::from_slice(&bytes[IPC_HEADER_LEN..meta_end]).map_err(|_| rejected())?;
        let projection: StageModelV1 =
            serde_json::from_slice(&bytes[meta_end..projection_end]).map_err(|_| rejected())?;
        let frame = Self {
            logical_id: LogicalInspectionIdV1::new(read_u64(bytes, 9)?),
            emission_id: InspectionEmissionIdV1::new(read_u64(bytes, 17)?),
            sequence: InspectionSequenceV1::new(read_u64(bytes, 25)?),
            epoch: InspectionEpochV1::new(read_u64(bytes, 33)?),
            stage_domain: decode_stage(bytes[41])?,
            meta,
            projection,
            safe_wire_bytes: meta_len + projection_len,
            payload: Zeroizing::new(bytes[projection_end..].to_vec()),
        };
        // The typed metadata must name the exact header identity, and an omitted payload
        // can never smuggle bytes.
        if frame.meta.logical_id != frame.logical_id
            || frame.meta.emission_id != frame.emission_id
            || frame.meta.sequence != frame.sequence
            || frame.meta.epoch != frame.epoch
            || frame.meta.stage_domain != frame.stage_domain
            || (matches!(
                frame.projection.payload(),
                PayloadAvailabilityV1::Omitted(_)
            ) && !frame.payload.is_empty())
        {
            return Err(rejected());
        }
        Ok(frame)
    }

    pub(crate) fn retained_bytes(&self) -> usize {
        IPC_HEADER_LEN + self.safe_wire_bytes + self.payload.len()
    }

    pub(crate) fn identical_to(&self, other: &Self) -> bool {
        self.logical_id == other.logical_id
            && self.emission_id == other.emission_id
            && self.sequence == other.sequence
            && self.epoch == other.epoch
            && self.stage_domain == other.stage_domain
            && self.meta == other.meta
            && self.projection == other.projection
            && self.payload.as_slice() == other.payload.as_slice()
    }

    /// Test constructor with consistent typed metadata.
    #[cfg(test)]
    pub(crate) fn for_test(
        logical: u64,
        emission: u64,
        sequence: u64,
        epoch: u64,
        stage_domain: InspectionStageDomainV1,
        payload: &[u8],
    ) -> Self {
        let meta = gaze_types::inspection::InspectionEventMetaV1::new(
            LogicalInspectionIdV1::new(logical),
            InspectionEmissionIdV1::new(emission),
            InspectionSequenceV1::new(sequence),
            InspectionEpochV1::new(epoch),
            stage_domain,
            crate::test_support::safe_fields(),
        );
        let meta: MetaModelV1 =
            serde_json::from_value(serde_json::to_value(meta).unwrap()).unwrap();
        let projection: StageModelV1 = serde_json::from_value(serde_json::json!({
            "payload": "present",
            "measurement": { "omitted": "not_applicable" },
            "json_shape": { "omitted": "not_applicable" },
            "sse_timeline": { "omitted": "not_applicable" },
            "pii_summary": { "omitted": "not_applicable" },
            "decision_trace": { "omitted": "not_applicable" },
            "attestation": { "omitted": "not_applicable" }
        }))
        .unwrap();
        Self {
            logical_id: LogicalInspectionIdV1::new(logical),
            emission_id: InspectionEmissionIdV1::new(emission),
            sequence: InspectionSequenceV1::new(sequence),
            epoch: InspectionEpochV1::new(epoch),
            stage_domain,
            meta,
            projection,
            safe_wire_bytes: 64,
            payload: Zeroizing::new(payload.to_vec()),
        }
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

    fn reencode(frame_bytes: &[u8], projection: &[u8], payload: &[u8]) -> Vec<u8> {
        let meta_len = read_u32(frame_bytes, 42).unwrap() as usize;
        let meta = &frame_bytes[IPC_HEADER_LEN..IPC_HEADER_LEN + meta_len];
        let total = IPC_HEADER_LEN + meta.len() + projection.len() + payload.len();
        let mut bytes = frame_bytes[..IPC_HEADER_LEN].to_vec();
        bytes[5..9].copy_from_slice(&(total as u32).to_be_bytes());
        bytes[46..50].copy_from_slice(&(projection.len() as u32).to_be_bytes());
        bytes[50..54].copy_from_slice(&(payload.len() as u32).to_be_bytes());
        bytes.extend_from_slice(meta);
        bytes.extend_from_slice(projection);
        bytes.extend_from_slice(payload);
        bytes
    }

    #[test]
    fn omitted_payload_frames_cannot_carry_payload_bytes() {
        use crate::test_support::{emit_logical, StageShape};
        use gaze_types::inspection::{CaptureDomainsV1, ProjectionOmissionReasonV1};
        let events = emit_logical(
            CaptureDomainsV1::All,
            &[(
                InspectionStageDomainV1::ProviderRequest,
                StageShape::PayloadOmitted(ProjectionOmissionReasonV1::LimitExceeded),
            )],
        );
        let encoded = EncodedInspectionFrame::from_event(&events[0], 1 << 20).unwrap();
        let decoded = DecodedInspectionFrame::decode(encoded.as_bytes(), 1 << 20).unwrap();
        assert!(decoded.payload.is_empty());
        let projection = serde_json::to_vec(&decoded.projection).unwrap();
        let smuggled = reencode(encoded.as_bytes(), &projection, b"<Email_1>");
        assert!(DecodedInspectionFrame::decode(&smuggled, 1 << 20).is_err());
        let unchanged = reencode(encoded.as_bytes(), &projection, b"");
        assert!(DecodedInspectionFrame::decode(&unchanged, 1 << 20).is_ok());
        let malformed = reencode(encoded.as_bytes(), br#"{"payload":"present"}"#, b"");
        assert!(DecodedInspectionFrame::decode(&malformed, 1 << 20).is_err());
    }

    #[test]
    fn typed_metadata_must_match_the_header_identity() {
        use crate::test_support::{emit_logical, StageShape};
        use gaze_types::inspection::CaptureDomainsV1;
        let events = emit_logical(
            CaptureDomainsV1::All,
            &[(
                InspectionStageDomainV1::ProviderResponse,
                StageShape::Present,
            )],
        );
        let encoded = EncodedInspectionFrame::from_event(&events[0], 1 << 20).unwrap();
        let mut bytes = encoded.as_bytes().to_vec();
        assert!(DecodedInspectionFrame::decode(&bytes, 1 << 20).is_ok());
        bytes[41] = stage_code(InspectionStageDomainV1::ProviderRequest);
        assert!(DecodedInspectionFrame::decode(&bytes, 1 << 20).is_err());
    }

    #[test]
    fn status_frames_round_trip_only_dashboard_descriptors() {
        use gaze_types::inspection::CaptureDomainsV1;
        for (domains, tier) in [
            (
                CaptureDomainsV1::ProviderVisible,
                CaptureTierV1::ProviderVisible,
            ),
            (
                CaptureDomainsV1::OwnerRawAndProviderVisible,
                CaptureTierV1::OwnerRaw,
            ),
            (
                CaptureDomainsV1::ProviderVisibleAndOwnerRestored,
                CaptureTierV1::OwnerRestored,
            ),
            (CaptureDomainsV1::All, CaptureTierV1::Both),
        ] {
            let bytes =
                RuntimeStatusFrameV1::encode(DashboardCaptureDescriptorV1::new(domains), 7, 2)
                    .unwrap();
            let Ok(InboundFrameV1::Status(status)) = InboundFrameV1::decode(&bytes, 1 << 20) else {
                panic!("status frame decodes");
            };
            assert_eq!(
                status,
                RuntimeStatusFrameV1 {
                    capture: tier,
                    ingress_full: 7,
                    ingress_closed: 2
                }
            );
            let mut bad = bytes;
            bad[5] = 9;
            assert!(InboundFrameV1::decode(&bad, 1 << 20).is_err());
        }
        for domains in [
            CaptureDomainsV1::MetadataOnly,
            CaptureDomainsV1::OwnerRaw,
            CaptureDomainsV1::OwnerRestored,
            CaptureDomainsV1::OwnerRawAndOwnerRestored,
        ] {
            assert!(
                RuntimeStatusFrameV1::encode(DashboardCaptureDescriptorV1::new(domains), 0, 0)
                    .is_err()
            );
        }
    }
}
