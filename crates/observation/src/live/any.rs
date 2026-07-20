//! Type-erased live transport wrappers for Gateway / Dashboard composition.

use crate::ObservationError;
use crate::live::message::LiveMessage;
use crate::live::traits::{LiveSink, LiveSource};
use crate::live::uds::{UdsLiveSink, UdsLiveSource};
use crate::live::zenoh::{ZenohLiveSink, ZenohLiveSource};

pub enum AnyLiveSink {
    Uds(UdsLiveSink),
    Zenoh(ZenohLiveSink),
}

impl LiveSink for AnyLiveSink {
    fn emit(&mut self, message: &LiveMessage) -> Result<(), ObservationError> {
        match self {
            Self::Uds(s) => s.emit(message),
            Self::Zenoh(s) => s.emit(message),
        }
    }

    fn finish(&mut self) -> Result<(), ObservationError> {
        match self {
            Self::Uds(s) => s.finish(),
            Self::Zenoh(s) => s.finish(),
        }
    }
}

pub enum AnyLiveSource {
    Uds(UdsLiveSource),
    Zenoh(ZenohLiveSource),
}

impl AnyLiveSource {
    pub async fn recv(&mut self) -> Result<Option<LiveMessage>, ObservationError> {
        match self {
            Self::Uds(s) => s.recv().await,
            Self::Zenoh(s) => s.recv().await,
        }
    }
}

impl LiveSource for AnyLiveSource {
    fn recv_blocking(&mut self) -> Result<Option<LiveMessage>, ObservationError> {
        let handle = tokio::runtime::Handle::current();
        handle.block_on(self.recv())
    }
}
