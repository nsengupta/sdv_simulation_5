//! Peer Zenoh live transport (one keyexpr; wait for first subscriber).

use std::time::Duration;

use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::timeout;
use zenoh::pubsub::Publisher;
use zenoh::sample::Sample;
use zenoh::Session;
use zenoh::Wait;

use crate::ObservationError;
use crate::live::message::LiveMessage;
use crate::live::traits::{LiveSink, LiveSource};

fn zenoh_err(err: impl ToString) -> ObservationError {
    ObservationError::Zenoh {
        message: err.to_string(),
    }
}

/// Gateway-side live sink: peer session, wait for a subscriber, put `hello`, then stream events.
pub struct ZenohLiveSink {
    tx: Option<mpsc::UnboundedSender<LiveMessage>>,
    writer: Option<JoinHandle<()>>,
    keyexpr: String,
}

impl ZenohLiveSink {
    /// Open a peer session, declare a publisher on `keyexpr`, wait until matching
    /// subscribers ≥ 1 (or timeout), put `hello`, return a sink ready for `emit`.
    pub async fn open_and_wait_subscriber(
        keyexpr: impl Into<String>,
        connect_timeout: Duration,
        hello: LiveMessage,
    ) -> Result<Self, ObservationError> {
        let keyexpr = keyexpr.into();
        let session = zenoh::open(zenoh::Config::default())
            .await
            .map_err(zenoh_err)?;
        let publisher = session
            .declare_publisher(keyexpr.clone())
            .await
            .map_err(zenoh_err)?;

        wait_for_matching_subscriber(&publisher, &keyexpr, connect_timeout).await?;

        let line = hello.to_json_line()?;
        publisher.put(line).await.map_err(zenoh_err)?;

        let (tx, rx) = mpsc::unbounded_channel();
        let writer = tokio::spawn(write_loop(session, publisher, rx));

        Ok(Self {
            tx: Some(tx),
            writer: Some(writer),
            keyexpr,
        })
    }

    pub fn keyexpr(&self) -> &str {
        &self.keyexpr
    }
}

async fn wait_for_matching_subscriber(
    publisher: &Publisher<'_>,
    keyexpr: &str,
    connect_timeout: Duration,
) -> Result<(), ObservationError> {
    let already = publisher.matching_status().await.map_err(zenoh_err)?;
    if already.matching() {
        return Ok(());
    }

    let listener = publisher.matching_listener().await.map_err(zenoh_err)?;
    timeout(connect_timeout, async {
        loop {
            let status = listener.recv_async().await.map_err(zenoh_err)?;
            if status.matching() {
                return Ok(());
            }
        }
    })
    .await
    .map_err(|_| ObservationError::Zenoh {
        message: format!(
            "timed out waiting for subscriber on {keyexpr} after {connect_timeout:?}"
        ),
    })?
}

async fn write_loop(
    session: Session,
    publisher: Publisher<'static>,
    mut rx: mpsc::UnboundedReceiver<LiveMessage>,
) {
    while let Some(message) = rx.recv().await {
        let Ok(line) = message.to_json_line() else {
            break;
        };
        if publisher.put(line).await.is_err() {
            break;
        }
    }
    let _ = session.close().await;
}

impl LiveSink for ZenohLiveSink {
    fn emit(&mut self, message: &LiveMessage) -> Result<(), ObservationError> {
        let Some(tx) = &self.tx else {
            return Ok(());
        };
        if tx.send(message.clone()).is_err() {
            self.tx = None;
        }
        Ok(())
    }

    fn finish(&mut self) -> Result<(), ObservationError> {
        self.tx = None;
        if let Some(handle) = self.writer.take() {
            handle.abort();
        }
        Ok(())
    }
}

impl Drop for ZenohLiveSink {
    fn drop(&mut self) {
        let _ = self.finish();
    }
}

/// Dashboard-side live source over a peer Zenoh subscriber.
pub struct ZenohLiveSource {
    session: Session,
    subscriber: zenoh::pubsub::Subscriber<zenoh::handlers::FifoChannelHandler<Sample>>,
    keyexpr: String,
    closed: bool,
}

impl ZenohLiveSource {
    pub async fn subscribe(keyexpr: impl Into<String>) -> Result<Self, ObservationError> {
        let keyexpr = keyexpr.into();
        let session = zenoh::open(zenoh::Config::default())
            .await
            .map_err(zenoh_err)?;
        let subscriber = session
            .declare_subscriber(keyexpr.clone())
            .await
            .map_err(zenoh_err)?;
        Ok(Self {
            session,
            subscriber,
            keyexpr,
            closed: false,
        })
    }

    pub async fn recv(&mut self) -> Result<Option<LiveMessage>, ObservationError> {
        if self.closed {
            return Ok(None);
        }
        match self.subscriber.recv_async().await {
            Ok(sample) => {
                let line = sample
                    .payload()
                    .try_to_string()
                    .map_err(|err| ObservationError::Zenoh {
                        message: format!("payload is not UTF-8: {err}"),
                    })?;
                Ok(Some(LiveMessage::from_json_line(line.trim_end())?))
            }
            Err(_) => {
                self.closed = true;
                Ok(None)
            }
        }
    }

    pub fn keyexpr(&self) -> &str {
        &self.keyexpr
    }
}

impl LiveSource for ZenohLiveSource {
    fn recv_blocking(&mut self) -> Result<Option<LiveMessage>, ObservationError> {
        let handle = tokio::runtime::Handle::current();
        handle.block_on(self.recv())
    }
}

impl Drop for ZenohLiveSource {
    fn drop(&mut self) {
        // Best-effort close; ignore errors during drop.
        let _ = self.session.close().wait();
    }
}
