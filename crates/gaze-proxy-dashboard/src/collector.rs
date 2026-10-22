// Kept compiled for the post-identity activation path; the public seam currently rejects closed.
#![allow(dead_code)]

use std::io::Write;
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TryRecvError, TrySendError};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use gaze_inspection::InspectionEventV1;
use gaze_types::inspection::DashboardCaptureDescriptorV1;

use crate::ipc::{EncodedInspectionFrame, RuntimeStatusFrameV1};
use crate::sink::Admission;
use crate::{DashboardError, DashboardErrorCode};

enum WriterCommand {
    Drain(SyncSender<()>),
    Stop(SyncSender<()>),
}

pub(crate) struct WriterHandle {
    commands: SyncSender<WriterCommand>,
    faulted: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl WriterHandle {
    pub(crate) fn spawn(
        receiver: Receiver<InspectionEventV1>,
        stream: UnixStream,
        frame_cap: usize,
        status: StatusSource,
    ) -> Result<Self, DashboardError> {
        let (commands, command_rx) = mpsc::sync_channel(16);
        let faulted = Arc::new(AtomicBool::new(false));
        let thread_faulted = faulted.clone();
        let thread = thread::Builder::new()
            .name("gaze-dashboard-writer".to_owned())
            .spawn(move || {
                writer_loop(
                    receiver,
                    command_rx,
                    stream,
                    frame_cap,
                    &status,
                    &thread_faulted,
                );
            })
            .map_err(|_| DashboardError::new(DashboardErrorCode::ActivationFailed))?;
        Ok(Self {
            commands,
            faulted,
            thread: Some(thread),
        })
    }

    pub(crate) fn drain(&self) -> Result<(), DashboardError> {
        let (ack_tx, ack_rx) = mpsc::sync_channel(1);
        self.commands
            .try_send(WriterCommand::Drain(ack_tx))
            .map_err(map_command_error)?;
        ack_rx
            .recv_timeout(Duration::from_secs(2))
            .map_err(|_| DashboardError::new(DashboardErrorCode::FatalDisabled))
    }

    pub(crate) fn is_faulted(&self) -> bool {
        self.faulted.load(Ordering::Acquire)
    }

    pub(crate) fn stop_and_join(&mut self) -> Result<(), DashboardError> {
        let (ack_tx, ack_rx) = mpsc::sync_channel(1);
        self.commands
            .try_send(WriterCommand::Stop(ack_tx))
            .map_err(map_command_error)?;
        ack_rx
            .recv_timeout(Duration::from_secs(2))
            .map_err(|_| DashboardError::new(DashboardErrorCode::FatalDisabled))?;
        if let Some(thread) = self.thread.take() {
            thread
                .join()
                .map_err(|_| DashboardError::new(DashboardErrorCode::FatalDisabled))?;
        }
        Ok(())
    }
}

fn map_command_error(error: TrySendError<WriterCommand>) -> DashboardError {
    match error {
        TrySendError::Full(_) => DashboardError::new(DashboardErrorCode::ControlQueueFull),
        TrySendError::Disconnected(_) => DashboardError::new(DashboardErrorCode::FatalDisabled),
    }
}

impl Drop for WriterHandle {
    fn drop(&mut self) {
        if self.thread.is_some() {
            let _ = self.stop_and_join();
        }
    }
}

/// Parent-side facts the child cannot observe: the immutable startup descriptor and the
/// bounded ingress drop counters.
pub(crate) struct StatusSource {
    pub(crate) descriptor: DashboardCaptureDescriptorV1,
    pub(crate) admission: Arc<Admission>,
}

impl StatusSource {
    fn counters(&self) -> (u64, u64) {
        let counters = self.admission.snapshot();
        (counters.full, counters.closed)
    }
}

fn write_frame(stream: &mut UnixStream, frame: &[u8]) -> bool {
    let Ok(length) = u32::try_from(frame.len()) else {
        return false;
    };
    stream.write_all(&length.to_be_bytes()).is_ok()
        && stream.write_all(frame).is_ok()
        && stream.flush().is_ok()
}

/// Writes a status frame when the reported counters changed. `None` forces a write.
fn write_status(
    stream: &mut UnixStream,
    status: &StatusSource,
    last: &mut Option<(u64, u64)>,
) -> bool {
    let current = status.counters();
    if *last == Some(current) {
        return true;
    }
    let Ok(frame) = RuntimeStatusFrameV1::encode(status.descriptor, current.0, current.1) else {
        return false;
    };
    *last = Some(current);
    write_frame(stream, &frame)
}

fn writer_loop(
    receiver: Receiver<InspectionEventV1>,
    commands: Receiver<WriterCommand>,
    mut stream: UnixStream,
    frame_cap: usize,
    status: &StatusSource,
    faulted: &AtomicBool,
) {
    let _ = stream.set_write_timeout(Some(Duration::from_secs(1)));
    // The descriptor is reported before any event can be written.
    let mut last_status = None;
    if !write_status(&mut stream, status, &mut last_status) {
        faulted.store(true, Ordering::Release);
        return;
    }
    loop {
        if !write_status(&mut stream, status, &mut last_status) {
            faulted.store(true, Ordering::Release);
            break;
        }
        match commands.try_recv() {
            Ok(WriterCommand::Drain(ack)) => {
                drain_events(&receiver);
                let _ = ack.send(());
                continue;
            }
            Ok(WriterCommand::Stop(ack)) => {
                drain_events(&receiver);
                let _ = stream.shutdown(Shutdown::Write);
                let _ = ack.send(());
                break;
            }
            Err(TryRecvError::Disconnected) => break,
            Err(TryRecvError::Empty) => {}
        }
        match receiver.recv_timeout(Duration::from_millis(10)) {
            Ok(event) => {
                let frame = match EncodedInspectionFrame::from_event(&event, frame_cap) {
                    Ok(frame) => frame,
                    Err(_) => continue,
                };
                if !write_frame(&mut stream, frame.as_bytes()) {
                    faulted.store(true, Ordering::Release);
                    break;
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
}

fn drain_events(receiver: &Receiver<InspectionEventV1>) {
    while receiver.try_recv().is_ok() {}
}
