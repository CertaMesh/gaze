use std::io::Write;
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use gaze_inspection::InspectionEventV1;

use crate::ipc::EncodedInspectionFrame;
use crate::{DashboardError, DashboardErrorCode};

enum WriterCommand {
    Drain(Sender<()>),
    Stop(Sender<()>),
}

pub(crate) struct WriterHandle {
    commands: Sender<WriterCommand>,
    faulted: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl WriterHandle {
    pub(crate) fn spawn(
        receiver: Receiver<InspectionEventV1>,
        stream: UnixStream,
        frame_cap: usize,
    ) -> Result<Self, DashboardError> {
        let (commands, command_rx) = mpsc::channel();
        let faulted = Arc::new(AtomicBool::new(false));
        let thread_faulted = faulted.clone();
        let thread = thread::Builder::new()
            .name("gaze-dashboard-writer".to_owned())
            .spawn(move || writer_loop(receiver, command_rx, stream, frame_cap, &thread_faulted))
            .map_err(|_| DashboardError::new(DashboardErrorCode::ActivationFailed))?;
        Ok(Self {
            commands,
            faulted,
            thread: Some(thread),
        })
    }

    pub(crate) fn drain(&self) -> Result<(), DashboardError> {
        let (ack_tx, ack_rx) = mpsc::channel();
        self.commands
            .send(WriterCommand::Drain(ack_tx))
            .map_err(|_| DashboardError::new(DashboardErrorCode::FatalDisabled))?;
        ack_rx
            .recv_timeout(Duration::from_secs(2))
            .map_err(|_| DashboardError::new(DashboardErrorCode::FatalDisabled))
    }

    pub(crate) fn is_faulted(&self) -> bool {
        self.faulted.load(Ordering::Acquire)
    }

    pub(crate) fn stop_and_join(&mut self) -> Result<(), DashboardError> {
        let (ack_tx, ack_rx) = mpsc::channel();
        self.commands
            .send(WriterCommand::Stop(ack_tx))
            .map_err(|_| DashboardError::new(DashboardErrorCode::FatalDisabled))?;
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

impl Drop for WriterHandle {
    fn drop(&mut self) {
        if self.thread.is_some() {
            let _ = self.stop_and_join();
        }
    }
}

fn writer_loop(
    receiver: Receiver<InspectionEventV1>,
    commands: Receiver<WriterCommand>,
    mut stream: UnixStream,
    frame_cap: usize,
    faulted: &AtomicBool,
) {
    let _ = stream.set_write_timeout(Some(Duration::from_secs(1)));
    loop {
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
                let length = u32::try_from(frame.as_bytes().len())
                    .expect("frame cap is below u32")
                    .to_be_bytes();
                if stream.write_all(&length).is_err()
                    || stream.write_all(frame.as_bytes()).is_err()
                    || stream.flush().is_err()
                {
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
