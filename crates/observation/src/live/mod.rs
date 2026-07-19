//! Detachable live observation transport (UDS now; Zenoh later).

mod memory;
mod message;
mod traits;
mod uds;

pub use memory::{MemoryLiveLink, MemoryLiveSink, MemoryLiveSource};
pub use message::{LiveMessage, LiveRecordDto, LiveStream};
pub use traits::{LiveSink, LiveSource};
pub use uds::{UdsLiveSink, UdsLiveSource};
