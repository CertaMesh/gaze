//! Deterministic protocol schedules; the original RED proof lives in commit677e648.
use super::*;
use crate::ipc::PairingEnvelopeV1;
use std::cell::RefCell;
use std::sync::mpsc::{self, Receiver, Sender};

const BOUND: Duration = Duration::from_secs(2);
type Gate = (Sender<Phase>, Receiver<()>);
#[derive(Debug, PartialEq)]
enum Phase {
    ChildPaused,
    ParentWaiting,
    ChildValidated,
    ParentReturned,
}
thread_local! {
    static BEFORE_TRAILING: RefCell<Option<Gate>> = const { RefCell::new(None) };
    static PHASES: RefCell<Option<Sender<Phase>>> = const { RefCell::new(None) };
}

pub(super) fn pause_before_trailing() {
    BEFORE_TRAILING.with(|slot| {
        if let Some((arrived, release)) = slot.borrow_mut().take() {
            arrived.send(Phase::ChildPaused).unwrap();
            release.recv_timeout(BOUND).expect("proof gate released");
        }
    });
}
pub(super) fn child_validated() {
    phase(Phase::ChildValidated);
}
pub(crate) fn parent_waiting() {
    phase(Phase::ParentWaiting);
}
fn phase(value: Phase) {
    PHASES.with(|slot| {
        if let Some(sender) = slot.borrow().as_ref() {
            sender.send(value).unwrap();
        }
    });
}
fn sockets() -> (UnixStream, UnixStream) {
    let pair = UnixStream::pair().unwrap();
    for socket in [&pair.0, &pair.1] {
        socket.set_read_timeout(Some(BOUND)).unwrap();
        socket.set_write_timeout(Some(BOUND)).unwrap();
    }
    pair
}
fn authority() -> SocketAddrV4 {
    "127.0.0.1:54321".parse().unwrap()
}
fn state(secret: &PairingSecret) -> Arc<Mutex<ChildState>> {
    Arc::new(Mutex::new(ChildState {
        store: EventStore::new(
            RetentionLimits::new(4, 4096, Duration::from_secs(30)).unwrap(),
            InspectionEpochV1::new(0),
        ),
        auth: AuthRegistry::new(secret, 2),
        reveals: RevealRegistry::new(Duration::from_secs(30)),
    }))
}
fn purge_frame() -> [u8; 9] {
    let mut frame = [0; 9];
    frame[0] = PARENT_PURGE;
    frame[1..].copy_from_slice(&1_u64.to_be_bytes());
    frame
}
fn acknowledge(control: &mut UnixStream) -> Result<SocketAddrV4, DashboardError> {
    crate::supervisor::acknowledge_pairing_for_proof(
        control,
        &mut |actual, token: &[u8]| {
            assert_eq!(actual, authority());
            assert_eq!(token.len(), 43);
            Ok(())
        },
        |actual| actual == authority(),
    )
}
fn purge_then_shutdown(parent: &mut UnixStream) {
    parent.write_all(&purge_frame()).unwrap();
    let mut ack = [0; 9];
    parent.read_exact(&mut ack).unwrap();
    assert_eq!(ack[0], CHILD_PURGED);
    assert_eq!(&ack[1..], &1_u64.to_be_bytes());
    parent.write_all(&[PARENT_SHUTDOWN]).unwrap();
    let mut ack = [0];
    parent.read_exact(&mut ack).unwrap();
    assert_eq!(ack, [CHILD_STOPPED]);
}

// The release is explicit. Completion events are recorded at actual validation and
// return boundaries, and only the parent that returned may send the next command.
fn immediate_commands_after_ready(rotate: bool) {
    let (mut parent, mut child) = sockets();
    let (phases_tx, phases_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let child_phases = phases_tx.clone();
    let child_worker = thread::spawn(move || {
        PHASES.with(|slot| *slot.borrow_mut() = Some(child_phases.clone()));
        BEFORE_TRAILING.with(|slot| *slot.borrow_mut() = Some((child_phases, release_rx)));
        let secret = PairingSecret::from_pairing_frame([7; 32]);
        let state = state(&secret);
        if !rotate {
            child_pair(&mut child, authority(), &secret).unwrap();
        }
        child_control_loop(&mut child, authority(), &state, &AtomicUsize::new(0))
    });
    let parent_worker = thread::spawn(move || {
        PHASES.with(|slot| *slot.borrow_mut() = Some(phases_tx));
        if rotate {
            crate::supervisor::rotate_pairing_for_proof(
                &parent,
                authority(),
                &mut |_, _: &[u8]| Ok(()),
            )
            .unwrap();
        } else {
            acknowledge(&mut parent).unwrap();
        }
        phase(Phase::ParentReturned);
        purge_then_shutdown(&mut parent);
        // Keep the stream alive until child command processing finishes.
        parent
    });
    let first = phases_rx.recv_timeout(BOUND).unwrap();
    let second = phases_rx.recv_timeout(BOUND).unwrap();
    assert!(matches!(
        (&first, &second),
        (Phase::ChildPaused, Phase::ParentWaiting) | (Phase::ParentWaiting, Phase::ChildPaused)
    ));
    release_tx.send(()).unwrap();
    assert_eq!(
        phases_rx.recv_timeout(BOUND).unwrap(),
        Phase::ChildValidated
    );
    assert_eq!(
        phases_rx.recv_timeout(BOUND).unwrap(),
        Phase::ParentReturned
    );
    let _parent = parent_worker.join().unwrap();
    assert!(child_worker.join().unwrap().is_ok());
}
#[test]
fn startup_ready_precedes_return_and_immediate_purge_shutdown() {
    immediate_commands_after_ready(false);
}
#[test]
fn rotate_ready_precedes_return_and_immediate_purge_shutdown() {
    immediate_commands_after_ready(true);
}

#[test]
fn premature_command_or_garbage_before_ready_is_rejected() {
    for next in [purge_frame().to_vec(), vec![0x42]] {
        let (mut parent, mut child) = sockets();
        let (arrived_tx, arrived_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let worker = thread::spawn(move || {
            BEFORE_TRAILING.with(|slot| *slot.borrow_mut() = Some((arrived_tx, release_rx)));
            child_pair(
                &mut child,
                authority(),
                &PairingSecret::from_pairing_frame([7; 32]),
            )
        });
        let envelope = PairingEnvelopeV2::read_exact(&mut parent).unwrap();
        DeliveredAckV2::write_to(&mut parent, envelope.nonce()).unwrap();
        assert_eq!(arrived_rx.recv_timeout(BOUND).unwrap(), Phase::ChildPaused);
        parent.write_all(&next).unwrap();
        release_tx.send(()).unwrap();
        assert_eq!(
            worker.join().unwrap().unwrap_err().code(),
            DashboardErrorCode::PairingFailed
        );
        assert!(PairingReadyV2::read_from(&mut parent, envelope.nonce()).is_err());
    }
}
#[test]
fn malformed_delivered_fields_and_truncation_fail_closed() {
    for offset in [0, 4, 5, 6, 22, 23] {
        let (mut parent, mut child) = sockets();
        let worker = thread::spawn(move || {
            child_pair(
                &mut child,
                authority(),
                &PairingSecret::from_pairing_frame([7; 32]),
            )
        });
        let envelope = PairingEnvelopeV2::read_exact(&mut parent).unwrap();
        let mut ack = Vec::new();
        DeliveredAckV2::write_to(&mut ack, envelope.nonce()).unwrap();
        if offset == 23 {
            ack.pop();
        } else {
            ack[offset] ^= 0xff;
        }
        parent.write_all(&ack).unwrap();
        if offset == 23 {
            parent.shutdown(Shutdown::Write).unwrap();
        }
        assert_eq!(
            worker.join().unwrap().unwrap_err().code(),
            DashboardErrorCode::PairingFailed
        );
        assert!(PairingReadyV2::read_from(&mut parent, envelope.nonce()).is_err());
    }
}

#[test]
fn parent_rejects_envelope_trailing_authority_and_delivery_failure_before_ack() {
    for case in 0..3 {
        let (mut parent, mut child) = sockets();
        let secret = PairingSecret::from_pairing_frame([7; 32]);
        PairingEnvelopeV2::encode([3; 16], authority(), &secret)
            .write_to(&mut child)
            .unwrap();
        if case == 0 {
            child.write_all(&[0x42]).unwrap();
        }
        let deliveries = Arc::new(AtomicUsize::new(0));
        let delivered = deliveries.clone();
        let error = crate::supervisor::acknowledge_pairing_for_proof(
            &mut parent,
            &mut move |_, _: &[u8]| {
                delivered.fetch_add(1, Ordering::SeqCst);
                Err(std::io::Error::other("synthetic delivery failure"))
            },
            |_| case != 1,
        )
        .unwrap_err();
        assert_eq!(error.code(), DashboardErrorCode::PairingFailed);
        assert_eq!(deliveries.load(Ordering::SeqCst), usize::from(case == 2));
        child.set_nonblocking(true).unwrap();
        assert_eq!(
            child.read(&mut [0]).unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
    }
}

// Parent must wait for Ready even after delivering the secret and writing Ack.
// A socket timeout is the actual bounded failure, not an absence inferred from sleep.
#[test]
fn missing_ready_times_out_instead_of_returning_success() {
    let (mut parent, mut child) = sockets();
    parent
        .set_read_timeout(Some(Duration::from_millis(30)))
        .unwrap();
    let (release_tx, release_rx) = mpsc::channel();
    let worker = thread::spawn(move || {
        PairingEnvelopeV2::encode(
            [3; 16],
            authority(),
            &PairingSecret::from_pairing_frame([7; 32]),
        )
        .write_to(&mut child)
        .unwrap();
        DeliveredAckV2::read_from(&mut child, [3; 16]).unwrap();
        release_rx.recv_timeout(BOUND).unwrap();
    });
    let result = acknowledge(&mut parent);
    release_tx.send(()).unwrap();
    worker.join().unwrap();
    assert_eq!(
        result.unwrap_err().code(),
        DashboardErrorCode::PairingFailed
    );
}

#[test]
fn malformed_ready_never_completes_parent_pairing() {
    // magic, version, kind/reflected Ack, nonce/stale nonce, status, truncation,
    // duplicate, trailing garbage, and EOF. Frames are queued in one complete write.
    for case in 0..9 {
        let (mut parent, mut child) = sockets();
        let (release_tx, release_rx) = mpsc::channel();
        let worker = thread::spawn(move || {
            PairingEnvelopeV2::encode(
                [3; 16],
                authority(),
                &PairingSecret::from_pairing_frame([7; 32]),
            )
            .write_to(&mut child)
            .unwrap();
            DeliveredAckV2::read_from(&mut child, [3; 16]).unwrap();
            let mut bytes = Vec::new();
            PairingReadyV2::write_to(&mut bytes, [3; 16]).unwrap();
            match case {
                0 => bytes[0] ^= 1,
                1 => bytes[4] = 1,
                2 => bytes[5] = 2,
                3 => bytes[6..22].fill(2),
                4 => bytes[22] = 0,
                5 => {
                    bytes.pop();
                }
                6 => bytes.extend_from_within(..),
                7 => bytes.push(0x42),
                8 => bytes.clear(),
                _ => unreachable!(),
            }
            child.write_all(&bytes).unwrap();
            if case == 5 || case == 8 {
                child.shutdown(Shutdown::Write).unwrap();
            }
            release_rx.recv_timeout(BOUND).unwrap();
        });
        let result = acknowledge(&mut parent);
        release_tx.send(()).unwrap();
        worker.join().unwrap();
        assert_eq!(
            result.unwrap_err().code(),
            DashboardErrorCode::PairingFailed,
            "case {case}"
        );
    }
}

#[test]
fn v1_child_is_rejected_by_runtime_parent_before_delivery() {
    let (mut parent, mut child) = sockets();
    PairingEnvelopeV1::encode(
        [3; 16],
        authority(),
        &PairingSecret::from_pairing_frame([7; 32]),
    )
    .write_to(&mut child)
    .unwrap();
    let result = crate::supervisor::acknowledge_pairing_for_proof(
        &mut parent,
        &mut |_, _: &[u8]| panic!("V1 must not deliver"),
        |_| true,
    );
    assert_eq!(
        result.unwrap_err().code(),
        DashboardErrorCode::PairingFailed
    );
}
#[test]
fn runtime_child_is_rejected_by_v1_parent_before_delivery() {
    let (mut parent, mut child) = sockets();
    let worker = thread::spawn(move || {
        child_pair(
            &mut child,
            authority(),
            &PairingSecret::from_pairing_frame([7; 32]),
        )
    });
    assert_eq!(
        PairingEnvelopeV1::read_exact(&mut parent)
            .err()
            .unwrap()
            .code(),
        DashboardErrorCode::PairingFailed
    );
    parent.shutdown(Shutdown::Both).unwrap();
    assert_eq!(
        worker.join().unwrap().unwrap_err().code(),
        DashboardErrorCode::PairingFailed
    );
}

#[test]
fn malformed_v2_envelope_header_authority_and_truncation_never_deliver() {
    for offset in [0, 4, 5, 22, 26, 60] {
        let (mut parent, mut child) = sockets();
        let mut bytes = Vec::new();
        PairingEnvelopeV2::encode(
            [3; 16],
            authority(),
            &PairingSecret::from_pairing_frame([7; 32]),
        )
        .write_to(&mut bytes)
        .unwrap();
        match offset {
            22 => bytes[22] = 8,
            26 => bytes[26..28].fill(0),
            60 => {
                bytes.pop();
            }
            _ => bytes[offset] ^= 0xff,
        }
        child.write_all(&bytes).unwrap();
        if offset == 60 {
            child.shutdown(Shutdown::Write).unwrap();
        }
        let result = crate::supervisor::acknowledge_pairing_for_proof(
            &mut parent,
            &mut |_, _: &[u8]| panic!("malformed envelope delivered"),
            |_| true,
        );
        assert_eq!(
            result.unwrap_err().code(),
            DashboardErrorCode::PairingFailed
        );
    }
}

#[test]
fn actual_parent_rotation_rejects_ready_replayed_from_previous_nonce() {
    let (mut parent, mut child) = sockets();
    let (release_tx, release_rx) = mpsc::channel();
    let worker = thread::spawn(move || {
        let secret = PairingSecret::from_pairing_frame([7; 32]);
        PairingEnvelopeV2::encode([3; 16], authority(), &secret)
            .write_to(&mut child)
            .unwrap();
        DeliveredAckV2::read_from(&mut child, [3; 16]).unwrap();
        PairingReadyV2::write_to(&mut child, [3; 16]).unwrap();
        let mut command = [0];
        child.read_exact(&mut command).unwrap();
        assert_eq!(command, [PARENT_ROTATE]);
        PairingEnvelopeV2::encode([4; 16], authority(), &secret)
            .write_to(&mut child)
            .unwrap();
        DeliveredAckV2::read_from(&mut child, [4; 16]).unwrap();
        PairingReadyV2::write_to(&mut child, [3; 16]).unwrap();
        release_rx.recv_timeout(BOUND).unwrap();
    });
    acknowledge(&mut parent).unwrap();
    let result = crate::supervisor::rotate_pairing_for_proof(
        &parent,
        authority(),
        &mut |_, _: &[u8]| Ok(()),
    );
    release_tx.send(()).unwrap();
    worker.join().unwrap();
    assert_eq!(
        result.unwrap_err().code(),
        DashboardErrorCode::PairingFailed
    );
}
