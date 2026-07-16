use anyhow::Result;
use socketcan::{CanFrame, CanSocket, Socket};

pub trait FrameSink {
    fn write_frame(&mut self, frame: CanFrame) -> Result<()>;
}

pub struct SocketCanSink {
    socket: CanSocket,
}

impl SocketCanSink {
    pub fn open(interface: &str) -> Result<Self> {
        Ok(Self {
            socket: CanSocket::open(interface)?,
        })
    }
}

impl FrameSink for SocketCanSink {
    fn write_frame(&mut self, frame: CanFrame) -> Result<()> {
        self.socket.write_frame(&frame)?;
        Ok(())
    }
}
