//! Modbus TCP transport — async or blocking stream I/O over `embedded_io_async` / `embedded_io`.

#[cfg(any(feature = "defmt", feature = "log"))]
use crate::mb_info;
use crate::{error::ModbusError, mb_error};
use core::convert::Infallible;

/// Modbus TCP framing driver.
///
/// `S` — stream implementing the I/O traits selected by the active feature.
/// `BUF` — frame buffer capacity in bytes.
pub(crate) struct ModbusTcp<S> {
    pub(crate) stream: S,
    pub(crate) transaction_id: u16,
}

impl<S> ModbusTcp<S> {
    pub(crate) fn new(stream: S) -> Self {
        Self {
            stream,
            transaction_id: 0,
        }
    }

    pub(crate) fn into_inner(self) -> S {
        self.stream
    }

    pub(crate) fn next_transaction_id(&mut self) -> u16 {
        self.transaction_id = self.transaction_id.wrapping_add(1);
        self.transaction_id
    }

}

// ── Async impl ────────────────────────────────────────────────────────────────

/// Maximum TCP frame size: 255-byte RTU PDU + 6-byte MBAP header.
const TCP_BUF: usize = 261;

#[cfg(feature = "async")]
impl<S> ModbusTcp<S>
where
    S: embedded_io_async::Read + embedded_io_async::Write,
{
    /// Read one complete Modbus TCP request frame using the MBAP PDU-length field.
    pub(crate) async fn listen(
        &mut self,
    ) -> Result<heapless::Vec<u8, TCP_BUF>, ModbusError<Infallible, S::Error>> {
        let mut mbap = [0u8; 6];
        self.read_exact(&mut mbap).await?;

        let pdu_len = u16::from_be_bytes([mbap[4], mbap[5]]) as usize;
        let mut req = heapless::Vec::<u8, TCP_BUF>::new();
        req.extend_from_slice(&mbap).map_err(|_| ModbusError::Push)?;

        let mut byte = [0u8; 1];
        for _ in 0..pdu_len {
            self.read_exact(&mut byte).await?;
            req.push(byte[0]).map_err(|_| ModbusError::Push)?;
        }

        #[cfg(feature = "defmt")]
        mb_info!("TCP RX req: {=[u8]:x}", req.as_slice());
        #[cfg(all(not(feature = "defmt"), feature = "log"))]
        mb_info!("TCP RX req: {} bytes", req.len());

        Ok(req)
    }

    /// Write raw bytes to the stream.
    pub(crate) async fn send(&mut self, data: &[u8]) -> Result<(), S::Error> {
        self.write_all(data).await?;
        #[cfg(feature = "defmt")]
        mb_info!("TCP TX: {=[u8]:x}", data);
        #[cfg(all(not(feature = "defmt"), feature = "log"))]
        mb_info!("TCP TX: {} bytes", data.len());
        Ok(())
    }

    async fn write_all(&mut self, data: &[u8]) -> Result<(), S::Error> {
        let mut remaining = data;
        while !remaining.is_empty() {
            let n = self.stream.write(remaining).await?;
            remaining = &remaining[n..];
        }
        Ok(())
    }

    async fn read_exact(
        &mut self,
        buf: &mut [u8],
    ) -> Result<(), ModbusError<Infallible, S::Error>> {
        let mut filled = 0;
        while filled < buf.len() {
            match self.stream.read(&mut buf[filled..]).await {
                Ok(0) => return Err(ModbusError::PayloadTooShort),
                Ok(n) => filled += n,
                Err(e) => {
                    mb_error!("TCP read error");
                    return Err(ModbusError::Tcp(e));
                }
            }
        }
        Ok(())
    }
}

// ── Sync (blocking) impl ──────────────────────────────────────────────────────

#[cfg(feature = "sync")]
impl<S> ModbusTcp<S>
where
    S: embedded_io::Read + embedded_io::Write,
{
    pub(crate) fn listen(
        &mut self,
    ) -> Result<heapless::Vec<u8, TCP_BUF>, ModbusError<Infallible, S::Error>> {
        let mut mbap = [0u8; 6];
        self.read_exact(&mut mbap)?;

        let pdu_len = u16::from_be_bytes([mbap[4], mbap[5]]) as usize;
        let mut req = heapless::Vec::<u8, TCP_BUF>::new();
        req.extend_from_slice(&mbap).map_err(|_| ModbusError::Push)?;

        let mut byte = [0u8; 1];
        for _ in 0..pdu_len {
            self.read_exact(&mut byte)?;
            req.push(byte[0]).map_err(|_| ModbusError::Push)?;
        }

        #[cfg(feature = "defmt")]
        mb_info!("TCP RX req: {=[u8]:x}", req.as_slice());
        #[cfg(all(not(feature = "defmt"), feature = "log"))]
        mb_info!("TCP RX req: {} bytes", req.len());

        Ok(req)
    }

    pub(crate) fn send(&mut self, data: &[u8]) -> Result<(), S::Error> {
        self.write_all(data)?;
        #[cfg(feature = "defmt")]
        mb_info!("TCP TX: {=[u8]:x}", data);
        #[cfg(all(not(feature = "defmt"), feature = "log"))]
        mb_info!("TCP TX: {} bytes", data.len());
        Ok(())
    }

    fn write_all(&mut self, data: &[u8]) -> Result<(), S::Error> {
        let mut remaining = data;
        while !remaining.is_empty() {
            let n = self.stream.write(remaining)?;
            remaining = &remaining[n..];
        }
        Ok(())
    }

    fn read_exact(
        &mut self,
        buf: &mut [u8],
    ) -> Result<(), ModbusError<Infallible, S::Error>> {
        let mut filled = 0;
        while filled < buf.len() {
            match self.stream.read(&mut buf[filled..]) {
                Ok(0) => return Err(ModbusError::PayloadTooShort),
                Ok(n) => filled += n,
                Err(e) => {
                    mb_error!("TCP read error");
                    return Err(ModbusError::Tcp(e));
                }
            }
        }
        Ok(())
    }
}
