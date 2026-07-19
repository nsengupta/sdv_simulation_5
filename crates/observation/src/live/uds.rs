//! Unix domain socket live transport (single client, no reconnect).

use std::path::{Path, PathBuf};
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::timeout;

use crate::ObservationError;
use crate::live::message::LiveMessage;
use crate::live::traits::{LiveSink, LiveSource};
use crate::uds_path::ensure_tmp_parent;

/// Gateway-side live sink: accepts one client, sends `hello`, then streams events.
pub struct UdsLiveSink {
    tx: Option<mpsc::UnboundedSender<LiveMessage>>,
    writer: Option<JoinHandle<()>>,
    path: PathBuf,
}

impl UdsLiveSink {
    /// Bind `path`, wait up to `connect_timeout` for one client, send `hello`, return sink.
    pub async fn bind_and_accept(
        path: impl Into<PathBuf>,
        connect_timeout: Duration,
        hello: LiveMessage,
    ) -> Result<Self, ObservationError> {
        let path = path.into();
        ensure_tmp_parent(&path)?;
        if path.exists() {
            std::fs::remove_file(&path).map_err(|source| ObservationError::Io {
                path: path.clone(),
                source,
            })?;
        }

        let listener = UnixListener::bind(&path).map_err(|source| ObservationError::Io {
            path: path.clone(),
            source,
        })?;

        let (mut stream, _) = timeout(connect_timeout, listener.accept())
            .await
            .map_err(|_| ObservationError::InvalidRecord {
                stream: path.clone(),
                line: 0,
                message: format!(
                    "timed out waiting for Dashboard on {} after {connect_timeout:?}",
                    path.display()
                ),
            })?
            .map_err(|source| ObservationError::Io {
                path: path.clone(),
                source,
            })?;

        write_message(&mut stream, &hello)
            .await
            .map_err(|()| ObservationError::Io {
                path: path.clone(),
                source: std::io::Error::new(std::io::ErrorKind::BrokenPipe, "failed to write hello"),
            })?;

        let (tx, rx) = mpsc::unbounded_channel();
        let writer = tokio::spawn(write_loop(stream, rx));

        Ok(Self {
            tx: Some(tx),
            writer: Some(writer),
            path,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

async fn write_loop(mut stream: UnixStream, mut rx: mpsc::UnboundedReceiver<LiveMessage>) {
    while let Some(message) = rx.recv().await {
        if write_message(&mut stream, &message).await.is_err() {
            break;
        }
    }
}

async fn write_message(stream: &mut UnixStream, message: &LiveMessage) -> Result<(), ()> {
    let line = message.to_json_line().map_err(|_| ())?;
    stream.write_all(line.as_bytes()).await.map_err(|_| ())?;
    stream.flush().await.map_err(|_| ())
}

impl LiveSink for UdsLiveSink {
    fn emit(&mut self, message: &LiveMessage) -> Result<(), ObservationError> {
        let Some(tx) = &self.tx else {
            return Ok(());
        };
        if tx.send(message.clone()).is_err() {
            // Client disconnected — ignore further live emits.
            self.tx = None;
        }
        Ok(())
    }

    fn finish(&mut self) -> Result<(), ObservationError> {
        self.tx = None;
        if let Some(handle) = self.writer.take() {
            // Best-effort: do not block the twin teardown on the writer task.
            handle.abort();
        }
        let _ = std::fs::remove_file(&self.path);
        Ok(())
    }
}

impl Drop for UdsLiveSink {
    fn drop(&mut self) {
        let _ = self.finish();
    }
}

/// Dashboard-side live source.
pub struct UdsLiveSource {
    reader: BufReader<UnixStream>,
    line: String,
    path: PathBuf,
}

impl UdsLiveSource {
    pub async fn connect(path: impl Into<PathBuf>) -> Result<Self, ObservationError> {
        let path = path.into();
        let stream = UnixStream::connect(&path)
            .await
            .map_err(|source| ObservationError::Io {
                path: path.clone(),
                source,
            })?;
        Ok(Self {
            reader: BufReader::new(stream),
            line: String::new(),
            path,
        })
    }

    pub async fn recv(&mut self) -> Result<Option<LiveMessage>, ObservationError> {
        self.line.clear();
        let bytes = self
            .reader
            .read_line(&mut self.line)
            .await
            .map_err(|source| ObservationError::Io {
                path: self.path.clone(),
                source,
            })?;
        if bytes == 0 {
            return Ok(None);
        }
        Ok(Some(LiveMessage::from_json_line(&self.line)?))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl LiveSource for UdsLiveSource {
    fn recv_blocking(&mut self) -> Result<Option<LiveMessage>, ObservationError> {
        let handle = tokio::runtime::Handle::current();
        handle.block_on(self.recv())
    }
}
