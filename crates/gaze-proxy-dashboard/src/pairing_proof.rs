//! Private deterministic source proof. No child process or no-dump bypass is involved.
use super::*;
use std::cell::RefCell;
use std::sync::mpsc::{self, Receiver, Sender};

const BOUND: Duration = Duration::from_secs(2);
type Gate = (Sender<()>, Receiver<()>);
thread_local! {
    static BEFORE_TRAILING: RefCell<Option<Gate>> = const { RefCell::new(None) };
}

pub(super) fn pause_before_trailing() {
    BEFORE_TRAILING.with(|slot| {
        if let Some((arrived, release)) = slot.borrow_mut().take() {
            arrived.send(()).unwrap();
            release.recv_timeout(BOUND).expect("proof gate released");
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

// The real child is paused AFTER validating the real parent's nonce-bound ack.
// Parent acknowledgement has returned; all next-frame bytes are queued BEFORE release.
fn queued_after_ack(rotate: bool, next: &[u8]) -> (Result<(), DashboardError>, Vec<u8>) {
    let (mut parent, mut child) = sockets();
    let (arrived_tx, arrived_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let worker = thread::spawn(move || {
        BEFORE_TRAILING.with(|slot| *slot.borrow_mut() = Some((arrived_tx, release_rx)));
        let secret = PairingSecret::from_pairing_frame([7; 32]);
        let state = state(&secret);
        let result = if rotate {
            child_control_loop(&mut child, authority(), &state, &AtomicUsize::new(0))
        } else {
            child_pair(&mut child, authority(), &secret)
        };
        child.set_nonblocking(true).unwrap();
        let mut remaining = Vec::new();
        let mut bytes = [0; 32];
        match child.read(&mut bytes) {
            Ok(n) => remaining.extend_from_slice(&bytes[..n]),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(error) => panic!("remaining-frame read: {error}"),
        }
        (result, remaining)
    });
    if rotate {
        parent.write_all(&[PARENT_ROTATE]).unwrap();
    }
    let deliveries = Arc::new(AtomicUsize::new(0));
    let delivered = deliveries.clone();
    crate::supervisor::acknowledge_pairing_for_proof(
        &mut parent,
        &mut move |actual, token: &[u8]| {
            assert_eq!(actual, authority());
            assert_eq!(token.len(), 43);
            delivered.fetch_add(1, Ordering::SeqCst);
            Ok(())
        },
        |actual| actual == authority(),
    )
    .unwrap();
    assert_eq!(deliveries.load(Ordering::SeqCst), 1);
    arrived_rx.recv_timeout(BOUND).expect("child validated ack");
    parent.write_all(next).unwrap();
    release_tx.send(()).unwrap();
    worker.join().unwrap()
}

#[test]
fn pairing_proof_red_startup_must_not_reject_queued_valid_purge() {
    let (result, _) = queued_after_ack(false, &purge_frame());
    assert!(
        result.is_ok(),
        "valid next command rejected after parent returned: {result:?}"
    );
}

#[test]
fn pairing_proof_red_rotate_must_not_reject_queued_valid_shutdown() {
    let (result, _) = queued_after_ack(true, &[PARENT_SHUTDOWN]);
    assert!(
        result.is_ok(),
        "valid shutdown rejected after rotate returned: {result:?}"
    );
}

#[test]
fn pairing_proof_characterizes_consumed_purge_opcode() {
    let frame = purge_frame();
    let (result, remaining) = queued_after_ack(false, &frame);
    assert_eq!(
        result.unwrap_err().code(),
        DashboardErrorCode::PairingFailed
    );
    assert_eq!(
        remaining,
        frame[1..],
        "trailing check consumed exactly the valid opcode"
    );
}

#[test]
fn pairing_proof_characterizes_rotate_purge_same_cut() {
    let frame = purge_frame();
    let (result, remaining) = queued_after_ack(true, &frame);
    assert_eq!(
        result.unwrap_err().code(),
        DashboardErrorCode::PairingFailed
    );
    assert_eq!(remaining, frame[1..]);
}

#[test]
fn pairing_proof_clean_ack_succeeds() {
    let (result, remaining) = queued_after_ack(false, &[]);
    assert!(result.is_ok());
    assert!(remaining.is_empty());
}

#[test]
fn pairing_proof_garbage_stays_rejected() {
    let (result, remaining) = queued_after_ack(false, &[0x42]);
    assert_eq!(
        result.unwrap_err().code(),
        DashboardErrorCode::PairingFailed
    );
    assert!(remaining.is_empty());
}

#[test]
fn pairing_proof_same_purge_frame_is_valid_in_control_phase() {
    let (mut parent, mut child) = sockets();
    let worker = thread::spawn(move || {
        let secret = PairingSecret::from_pairing_frame([7; 32]);
        child_control_loop(
            &mut child,
            authority(),
            &state(&secret),
            &AtomicUsize::new(0),
        )
    });
    parent.write_all(&purge_frame()).unwrap();
    let mut ack = [0; 9];
    parent.read_exact(&mut ack).unwrap();
    assert_eq!(ack[0], CHILD_PURGED);
    assert_eq!(&ack[1..], &1_u64.to_be_bytes());
    parent.write_all(&[PARENT_SHUTDOWN]).unwrap();
    let mut stopped = [0];
    parent.read_exact(&mut stopped).unwrap();
    assert_eq!(stopped, [CHILD_STOPPED]);
    assert!(worker.join().unwrap().is_ok());
}

#[test]
fn pairing_proof_malformed_ack_fields_stay_rejected() {
    for offset in [0, 4, 5, 21] {
        let (mut parent, mut child) = sockets();
        let worker = thread::spawn(move || {
            child_pair(
                &mut child,
                authority(),
                &PairingSecret::from_pairing_frame([7; 32]),
            )
        });
        let envelope = PairingEnvelopeV1::read_exact(&mut parent).unwrap();
        let mut ack = Vec::new();
        crate::DeliveredAckV1::delivered(envelope.nonce())
            .write_to(&mut ack)
            .unwrap();
        ack[offset] ^= 0xff;
        parent.write_all(&ack).unwrap();
        assert_eq!(
            worker.join().unwrap().unwrap_err().code(),
            DashboardErrorCode::PairingFailed
        );
    }
}

#[test]
fn pairing_proof_parent_rejects_envelope_trailing_authority_and_delivery_failure() {
    for case in 0..3 {
        let (mut parent, mut child) = sockets();
        let secret = PairingSecret::from_pairing_frame([7; 32]);
        PairingEnvelopeV1::encode([3; 16], authority(), &secret)
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
