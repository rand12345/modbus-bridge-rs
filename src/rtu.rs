//! Modbus RTU transport — async or blocking serial I/O over `embedded_io_async` / `embedded_io`.

#[cfg(any(feature = "defmt", feature = "log"))]
use crate::mb_info;
use crate::{
    error::ModbusError,
    frame::{self, RtuResponse},
    mb_error,
};
use core::convert::Infallible;
use embedded_hal::digital::OutputPin;

/// Modbus RTU driver.
///
/// `S` — serial port implementing the I/O traits selected by the active feature.
/// `TX` — RS-485 transmit-enable pin.
/// `BUF` — frame buffer capacity in bytes (default: [`TCP_MAX`](crate::capacity::TCP_MAX)).
///
/// Use [`rtu_capacity`](crate::capacity::rtu_capacity) or the named `RTU_*` constants to
/// right-size the buffer for your application.
pub(crate) struct ModbusRtu<S, TX> {
    pub(crate) serial: S,
    pub(crate) tx_en: TX,
}

impl<S, TX> ModbusRtu<S, TX> {
    pub(crate) fn new(serial: S, tx_en: TX) -> Self {
        Self { serial, tx_en }
    }

    pub(crate) fn into_inner(self) -> (S, TX) {
        (self.serial, self.tx_en)
    }
}

// ── Async impl ────────────────────────────────────────────────────────────────

/// Maximum RTU frame size (255 B): 123 registers × 2 bytes + 9 bytes overhead.
const RTU_BUF: usize = 255;

#[cfg(feature = "async")]
impl<S, TX> ModbusRtu<S, TX>
where
    S: embedded_io_async::Read + embedded_io_async::Write,
    TX: OutputPin,
{
    /// Transmit bytes over RS-485, driving TX enable around the write.
    pub(crate) async fn send(&mut self, payload: &[u8]) -> Result<(), S::Error> {
        self.flush().await?;
        let _ = self.tx_en.set_high();
        self.write_all(payload).await?;
        self.flush().await?;
        let _ = self.tx_en.set_low();
        #[cfg(feature = "defmt")]
        mb_info!("RTU TX: {=[u8]:x}", payload);
        #[cfg(all(not(feature = "defmt"), feature = "log"))]
        mb_info!("RTU TX: {} bytes", payload.len());
        Ok(())
    }

    /// Read one complete RTU request frame and verify CRC.
    ///
    /// Handles fixed-length FCs (8 bytes) and variable-length FCs (0x0F / 0x10).
    pub(crate) async fn listen(
        &mut self,
    ) -> Result<heapless::Vec<u8, RTU_BUF>, ModbusError<S::Error, Infallible>> {
        let mut req = heapless::Vec::<u8, RTU_BUF>::new();

        // Read addr + FC
        let mut hdr = [0u8; 2];
        self.read_exact(&mut hdr).await?;
        req.extend_from_slice(&hdr).map_err(|_| ModbusError::Push)?;

        match hdr[1] {
            // Write Multiple Coils / Write Multiple Registers — variable length
            0x0F | 0x10 => {
                // [start_hi, start_lo, qty_hi, qty_lo, byte_count]
                let mut fixed = [0u8; 5];
                self.read_exact(&mut fixed).await?;
                req.extend_from_slice(&fixed)
                    .map_err(|_| ModbusError::Push)?;
                let byte_count = fixed[4] as usize;
                // data bytes + 2 CRC bytes
                let mut byte = [0u8; 1];
                for _ in 0..(byte_count + 2) {
                    self.read_exact(&mut byte).await?;
                    req.push(byte[0]).map_err(|_| ModbusError::Push)?;
                }
            }
            // All other FCs: fixed 8-byte frame
            _ => {
                let mut rest = [0u8; 6];
                self.read_exact(&mut rest).await?;
                req.extend_from_slice(&rest)
                    .map_err(|_| ModbusError::Push)?;
            }
        }

        frame::check_crc(&req).map_err(|e| e.into_combined())?;

        #[cfg(feature = "defmt")]
        mb_info!("RTU RX req: {=[u8]:x}", req.as_slice());
        #[cfg(all(not(feature = "defmt"), feature = "log"))]
        mb_info!("RTU RX req: {} bytes", req.len());

        Ok(req)
    }

    /// Transmit `req` then read back the variable-length RTU response and verify CRC.
    pub(crate) async fn send_and_receive(
        &mut self,
        req: &[u8],
    ) -> Result<RtuResponse, ModbusError<S::Error, Infallible>> {
        self.send(req).await.map_err(ModbusError::Serial)?;
        self.receive().await
    }

    async fn receive(&mut self) -> Result<RtuResponse, ModbusError<S::Error, Infallible>> {
        let mut header = [0u8; 3];
        self.read_exact(&mut header).await?;

        let remaining = frame::rtu_response_remaining(&header);
        let mut resp = RtuResponse::new();
        resp.extend_from_slice(&header)
            .map_err(|_| ModbusError::Push)?;

        let mut byte = [0u8; 1];
        for _ in 0..remaining {
            self.read_exact(&mut byte).await?;
            resp.push(byte[0]).map_err(|_| ModbusError::Push)?;
        }

        #[cfg(feature = "defmt")]
        mb_info!("RTU RX resp: {=[u8]:x}", resp.as_slice());
        #[cfg(all(not(feature = "defmt"), feature = "log"))]
        mb_info!("RTU RX resp: {} bytes", resp.len());

        frame::check_crc(&resp).map_err(|e| e.into_combined())?;
        Ok(resp)
    }

    async fn write_all(&mut self, data: &[u8]) -> Result<(), S::Error> {
        let mut remaining = data;
        while !remaining.is_empty() {
            let n = self.serial.write(remaining).await?;
            remaining = &remaining[n..];
        }
        Ok(())
    }

    async fn flush(&mut self) -> Result<(), S::Error> {
        self.serial.flush().await
    }

    async fn read_exact(
        &mut self,
        buf: &mut [u8],
    ) -> Result<(), ModbusError<S::Error, Infallible>> {
        let mut filled = 0;
        let mut attempt = 1;
        while filled < buf.len() {
            match self.serial.read(&mut buf[filled..]).await {
                Ok(0) => return Err(ModbusError::PayloadTooShort),
                Ok(n) => filled += n,
                Err(e) => {
                    #[cfg(all(not(feature = "defmt"), feature = "log"))]
                    mb_error!("RTU read error {e} attempt {attempt}/10");
                    if attempt > 10 {
                        return Err(ModbusError::Serial(e));
                    } else {
                        attempt += 1;
                    }
                }
            }
        }
        Ok(())
    }
}

// ── Sync (blocking) impl ──────────────────────────────────────────────────────

#[cfg(feature = "sync")]
impl<S, TX> ModbusRtu<S, TX>
where
    S: embedded_io::Read + embedded_io::Write,
    TX: OutputPin,
{
    pub(crate) fn send(&mut self, payload: &[u8]) -> Result<(), S::Error> {
        self.flush()?;
        let _ = self.tx_en.set_high();
        self.write_all(payload)?;
        self.flush()?;
        let _ = self.tx_en.set_low();
        #[cfg(feature = "defmt")]
        mb_info!("RTU TX: {=[u8]:x}", payload);
        #[cfg(all(not(feature = "defmt"), feature = "log"))]
        mb_info!("RTU TX: {} bytes", payload.len());
        Ok(())
    }

    pub(crate) fn listen(
        &mut self,
    ) -> Result<heapless::Vec<u8, RTU_BUF>, ModbusError<S::Error, Infallible>> {
        let mut req = heapless::Vec::<u8, RTU_BUF>::new();

        let mut hdr = [0u8; 2];
        self.read_exact(&mut hdr)?;
        req.extend_from_slice(&hdr).map_err(|_| ModbusError::Push)?;

        match hdr[1] {
            0x0F | 0x10 => {
                let mut fixed = [0u8; 5];
                self.read_exact(&mut fixed)?;
                req.extend_from_slice(&fixed)
                    .map_err(|_| ModbusError::Push)?;
                let byte_count = fixed[4] as usize;
                let mut byte = [0u8; 1];
                for _ in 0..(byte_count + 2) {
                    self.read_exact(&mut byte)?;
                    req.push(byte[0]).map_err(|_| ModbusError::Push)?;
                }
            }
            _ => {
                let mut rest = [0u8; 6];
                self.read_exact(&mut rest)?;
                req.extend_from_slice(&rest)
                    .map_err(|_| ModbusError::Push)?;
            }
        }

        frame::check_crc(&req).map_err(|e| e.into_combined())?;

        #[cfg(feature = "defmt")]
        mb_info!("RTU RX req: {=[u8]:x}", req.as_slice());
        #[cfg(all(not(feature = "defmt"), feature = "log"))]
        mb_info!("RTU RX req: {} bytes", req.len());

        Ok(req)
    }

    pub(crate) fn send_and_receive(
        &mut self,
        req: &[u8],
    ) -> Result<RtuResponse, ModbusError<S::Error, Infallible>> {
        self.send(req).map_err(ModbusError::Serial)?;
        self.receive()
    }

    fn receive(&mut self) -> Result<RtuResponse, ModbusError<S::Error, Infallible>> {
        let mut header = [0u8; 3];
        self.read_exact(&mut header)?;

        let remaining = frame::rtu_response_remaining(&header);
        let mut resp = RtuResponse::new();
        resp.extend_from_slice(&header)
            .map_err(|_| ModbusError::Push)?;

        let mut byte = [0u8; 1];
        for _ in 0..remaining {
            self.read_exact(&mut byte)?;
            resp.push(byte[0]).map_err(|_| ModbusError::Push)?;
        }

        #[cfg(feature = "defmt")]
        mb_info!("RTU RX resp: {=[u8]:x}", resp.as_slice());
        #[cfg(all(not(feature = "defmt"), feature = "log"))]
        mb_info!("RTU RX resp: {} bytes", resp.len());

        frame::check_crc(&resp).map_err(|e| e.into_combined())?;
        Ok(resp)
    }

    fn write_all(&mut self, data: &[u8]) -> Result<(), S::Error> {
        let mut remaining = data;
        while !remaining.is_empty() {
            let n = self.serial.write(remaining)?;
            remaining = &remaining[n..];
        }
        Ok(())
    }

    fn flush(&mut self) -> Result<(), S::Error> {
        self.serial.flush()
    }

    fn read_exact(&mut self, buf: &mut [u8]) -> Result<(), ModbusError<S::Error, Infallible>> {
        let mut filled = 0;
        while filled < buf.len() {
            match self.serial.read(&mut buf[filled..]) {
                Ok(0) => return Err(ModbusError::PayloadTooShort),
                Ok(n) => filled += n,
                Err(e) => {
                    mb_error!("RTU read error");
                    return Err(ModbusError::Serial(e));
                }
            }
        }
        Ok(())
    }
}
