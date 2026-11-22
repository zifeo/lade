//! Tunnel connections carried over the link as frames: both ends pump one
//! socket per stream id, and the link writer sends control frames first.

use base64::Engine;
use lade_sdk::upstream::{Frame, write_frame};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::{Semaphore, mpsc};

const CHUNK: usize = 32 * 1024;
/// Stream frames queued toward the transport: a fast socket waits on a
/// slow link instead of filling memory.
const DATA_QUEUE: usize = 64;
/// `Data` frames a sender may have in flight on one stream before the
/// receiver acks them: 2 MiB. The link reader then never waits on one slow
/// socket, so no stream holds the others back.
// § aswq5qe7f.e@kpp2n
const WINDOW: u32 = 64;
// § end aswq5qe7f.e
/// The receiver acks once this many frames were written, or its queue ran dry.
const ACK_EVERY: u32 = WINDOW / 4;

#[derive(Clone)]
pub(crate) struct Out {
    control: mpsc::UnboundedSender<Frame>,
    data: mpsc::Sender<Frame>,
}

impl Out {
    /// The writer ends after a [`Frame::Detach`], or once every sender is gone.
    pub(crate) fn spawn<W>(mut writer: W) -> (Self, tokio::task::JoinHandle<()>)
    where
        W: AsyncWrite + Unpin + Send + 'static,
    {
        let (control, mut control_rx) = mpsc::unbounded_channel::<Frame>();
        let (data, mut data_rx) = mpsc::channel::<Frame>(DATA_QUEUE);
        let task = tokio::spawn(async move {
            loop {
                let frame = tokio::select! {
                    biased;
                    frame = control_rx.recv() => frame,
                    frame = data_rx.recv() => frame,
                };
                let Some(frame) = frame else { break };
                let detach = frame == Frame::Detach;
                if write_frame(&mut writer, &frame).await.is_err() || detach {
                    break;
                }
            }
        });
        (Self { control, data }, task)
    }

    pub(crate) fn send(&self, frame: Frame) -> bool {
        self.control.send(frame).is_ok()
    }

    async fn data(&self, frame: Frame) -> bool {
        self.data.send(frame).await.is_ok()
    }
}

pub(crate) enum Inbound {
    Data(Vec<u8>),
    End,
}

struct Entry {
    inbound: mpsc::Sender<Inbound>,
    credit: Arc<Semaphore>,
}

/// One stream's side of the registry, handed to [`Streams::pump`].
pub(crate) struct Opened {
    inbound: mpsc::Receiver<Inbound>,
    credit: Arc<Semaphore>,
}

/// The sockets of open streams, by stream id.
#[derive(Clone, Default)]
pub(crate) struct Streams(Arc<Mutex<HashMap<u64, Entry>>>);

impl Streams {
    pub(crate) fn open(&self, id: u64) -> Opened {
        // One slot past the window, for the End.
        let (tx, rx) = mpsc::channel(WINDOW as usize + 1);
        let credit = Arc::new(Semaphore::new(WINDOW as usize));
        let entry = Entry {
            inbound: tx,
            credit: Arc::clone(&credit),
        };
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id, entry);
        Opened {
            inbound: rx,
            credit,
        }
    }

    /// Ends stream `id` here: its pump then resets it toward the far end.
    pub(crate) fn close(&self, id: u64) {
        if let Some(entry) = self.0.lock().unwrap_or_else(|e| e.into_inner()).remove(&id) {
            entry.credit.close();
        }
    }

    /// Routes a stream frame from the link, without waiting on any socket.
    /// False when `frame` is not one. A far end past its window, or one that
    /// acks more than it was sent, gets its stream closed.
    pub(crate) fn route(&self, frame: &Frame) -> bool {
        let id = match frame {
            Frame::Data { id, .. }
            | Frame::End { id }
            | Frame::Reset { id }
            | Frame::Ack { id, .. } => *id,
            _ => return false,
        };
        let ok = {
            let map = self.0.lock().unwrap_or_else(|e| e.into_inner());
            let Some(entry) = map.get(&id) else {
                return true;
            };
            match frame {
                // The last slot is the End's.
                Frame::Data { data, .. } => {
                    entry.inbound.capacity() > 1
                        && base64::engine::general_purpose::STANDARD
                            .decode(data)
                            .is_ok_and(|bytes| entry.inbound.try_send(Inbound::Data(bytes)).is_ok())
                }
                Frame::End { .. } => entry.inbound.try_send(Inbound::End).is_ok(),
                Frame::Ack { n, .. } => {
                    let n = *n as usize;
                    let fits = entry.credit.available_permits() + n <= WINDOW as usize;
                    if fits {
                        entry.credit.add_permits(n);
                    }
                    fits
                }
                _ => false,
            }
        };
        if !ok {
            self.close(id);
        }
        true
    }

    /// Carries `io` both ways as stream `id` until both sides ended, or
    /// either fails, which resets the stream.
    pub(crate) async fn pump<S>(&self, io: S, id: u64, opened: Opened, out: Out)
    where
        S: AsyncRead + AsyncWrite + Send + 'static,
    {
        let Opened {
            mut inbound,
            credit,
        } = opened;
        let (mut read, mut write) = tokio::io::split(io);
        let up = {
            let out = out.clone();
            async move {
                let mut buf = vec![0u8; CHUNK];
                loop {
                    match read.read(&mut buf).await {
                        Ok(0) => return out.data(Frame::End { id }).await,
                        Ok(n) => {
                            let Ok(permit) = credit.acquire().await else {
                                return false;
                            };
                            permit.forget();
                            let data = base64::engine::general_purpose::STANDARD.encode(&buf[..n]);
                            if !out.data(Frame::Data { id, data }).await {
                                return false;
                            }
                        }
                        Err(_) => return false,
                    }
                }
            }
        };
        let down = {
            let out = out.clone();
            async move {
                let mut written = 0u32;
                while let Some(next) = inbound.recv().await {
                    match next {
                        Inbound::Data(bytes) => {
                            if write.write_all(&bytes).await.is_err() {
                                return false;
                            }
                            written += 1;
                            if written >= ACK_EVERY || inbound.is_empty() {
                                out.send(Frame::Ack { id, n: written });
                                written = 0;
                            }
                        }
                        Inbound::End => return write.shutdown().await.is_ok(),
                    }
                }
                false
            }
        };
        tokio::pin!(up, down);
        let (mut up_ok, mut down_ok) = (None, None);
        // A failed side ends the other: a reset socket must not keep reading.
        while up_ok.is_none() || down_ok.is_none() {
            tokio::select! {
                ok = &mut up, if up_ok.is_none() => up_ok = Some(ok),
                ok = &mut down, if down_ok.is_none() => down_ok = Some(ok),
            }
            if up_ok == Some(false) || down_ok == Some(false) {
                break;
            }
        }
        self.close(id);
        if up_ok != Some(true) || down_ok != Some(true) {
            let _ = out.data(Frame::Reset { id }).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data(id: u64) -> Frame {
        Frame::Data {
            id,
            data: base64::engine::general_purpose::STANDARD.encode(b"x"),
        }
    }

    fn is_open(streams: &Streams, id: u64) -> bool {
        streams.0.lock().unwrap().contains_key(&id)
    }

    #[test]
    fn a_far_end_past_its_window_loses_only_its_stream() {
        let streams = Streams::default();
        let _slow = streams.open(1);
        let _other = streams.open(2);
        for _ in 0..WINDOW {
            assert!(streams.route(&data(1)));
        }
        assert!(is_open(&streams, 1));
        // Nothing read the first socket: the link reader still moves on.
        streams.route(&data(1));
        assert!(!is_open(&streams, 1));
        streams.route(&data(2));
        assert!(is_open(&streams, 2));
    }

    #[test]
    fn an_ack_past_what_was_sent_closes_the_stream() {
        let streams = Streams::default();
        let _opened = streams.open(1);
        streams.route(&Frame::Ack { id: 1, n: 1 });
        assert!(!is_open(&streams, 1));
    }
}
