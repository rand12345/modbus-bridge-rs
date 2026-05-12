//! [`Connection`] — a live TCP session bound to a [`Bridge`](crate::Bridge).

use crate::{
    bridge::Bridge,
    error::ModbusError,
    event::{BridgeError, BridgeEvent, FunctionCode, Transaction, Warning},
    frame,
    tcp::ModbusTcp,
    NoDelay,
};
use embedded_hal::digital::OutputPin;

/// An active Modbus TCP → RTU bridge connection.
///
/// Returned by [`Bridge::accept`](crate::Bridge::accept). Mutably borrows the
/// bridge for its lifetime, preventing a second connection from being accepted
/// until this one is finished.
///
/// Drive the connection by calling [`next`](Connection::next) in a loop.
pub struct Connection<'b, S, TX, TS, D = NoDelay> {
    pub(crate) bridge: &'b mut Bridge<S, TX, D>,
    pub(crate) tcp: ModbusTcp<TS>,
}

impl<'b, S, TX, TS, D> Connection<'b, S, TX, TS, D> {
    pub(crate) fn new(bridge: &'b mut Bridge<S, TX, D>, stream: TS) -> Self {
        Self { bridge, tcp: ModbusTcp::new(stream) }
    }

    /// Consumes the connection and returns the underlying TCP stream.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let socket = conn.into_stream();
    /// socket.close();
    /// ```
    pub fn into_stream(self) -> TS {
        self.tcp.into_inner()
    }
}

// ── Async next() — no timeout ─────────────────────────────────────────────────

#[cfg(feature = "async")]
impl<S, TX, TS> Connection<'_, S, TX, TS, NoDelay>
where
    S: embedded_io_async::Read + embedded_io_async::Write,
    TX: OutputPin,
    TS: embedded_io_async::Read + embedded_io_async::Write,
{
    /// Drives one complete Modbus request/response cycle asynchronously.
    ///
    /// # Errors
    ///
    /// Returns [`BridgeError::TcpClosed`](crate::BridgeError::TcpClosed) when the
    /// TCP client closes the connection cleanly.
    pub async fn next(&mut self) -> Result<BridgeEvent, BridgeError<S::Error, TS::Error>> {
        let tcp_req = self.tcp.listen().await.map_err(|e| match e {
            ModbusError::PayloadTooShort => BridgeError::TcpClosed,
            ModbusError::Tcp(te) => BridgeError::TcpIo(te),
            ModbusError::Push => BridgeError::BufferOverflow,
            _ => BridgeError::BufferOverflow,
        })?;

        let (unit_id, fc_byte, start_address, register_count) =
            frame::parse_tcp_request(&tcp_req).unwrap_or((0, 0, 0, 0));

        let (rtu_req, tid) =
            frame::tcp_to_rtu(&tcp_req).map_err(|_| BridgeError::BufferOverflow)?;

        let rtu_resp = self.bridge.rtu.send_and_receive(&rtu_req).await
            .map_err(|e| match e {
                ModbusError::Serial(se) => BridgeError::RtuIo(se),
                ModbusError::Crc => BridgeError::RtuCrcMismatch,
                ModbusError::PayloadTooShort => BridgeError::RtuCrcMismatch,
                _ => BridgeError::BufferOverflow,
            })?;

        let (tcp_resp, tid_warning) = match frame::rtu_resp_to_tcp(&rtu_resp, tid) {
            Ok(r) => (r, None),
            Err(ModbusError::InvalidTransactionId) => {
                let fallback = frame::rtu_resp_to_tcp(&rtu_resp, 0)
                    .map_err(|_| BridgeError::BufferOverflow)?;
                let rx_tid = rtu_resp.get(..2)
                    .map(|b| u16::from_be_bytes([b[0], b[1]]))
                    .unwrap_or(0);
                (fallback, Some(Warning::TransactionIdMismatch { expected: tid, got: rx_tid }))
            }
            Err(_) => return Err(BridgeError::BufferOverflow),
        };

        self.tcp.send(&tcp_resp).await.map_err(BridgeError::TcpIo)?;

        if let Some(w) = tid_warning {
            return Ok(BridgeEvent::Warning(w));
        }
        Ok(BridgeEvent::Transaction(Transaction {
            unit_id,
            function_code: FunctionCode::from(fc_byte),
            start_address,
            register_count,
        }))
    }
}

// ── Async next() — with timeout ───────────────────────────────────────────────

#[cfg(feature = "async")]
impl<S, TX, TS, D> Connection<'_, S, TX, TS, D>
where
    S: embedded_io_async::Read + embedded_io_async::Write,
    TX: OutputPin,
    TS: embedded_io_async::Read + embedded_io_async::Write,
    D: embedded_hal_async::delay::DelayNs,
{
    /// Drives one complete Modbus request/response cycle asynchronously, with timeouts.
    ///
    /// Applies `tcp_timeout_ms` around reading the incoming TCP request and
    /// `rtu_timeout_ms` around the RTU send+receive cycle.
    ///
    /// # Errors
    ///
    /// Returns [`BridgeError::Timeout`](crate::BridgeError::Timeout) if a deadline expires.
    pub async fn next(&mut self) -> Result<BridgeEvent, BridgeError<S::Error, TS::Error>> {
        use core::pin::pin;
        use futures_util::future::{select, Either};

        let tcp_req = if let Some(ms) = self.bridge.tcp_timeout_ms {
            let tcp_fut = pin!(self.tcp.listen());
            let delay_fut = pin!(self.bridge.delay.delay_ms(ms));
            match select(tcp_fut, delay_fut).await {
                Either::Left((r, _)) => r.map_err(|e| match e {
                    ModbusError::PayloadTooShort => BridgeError::TcpClosed,
                    ModbusError::Tcp(te) => BridgeError::TcpIo(te),
                    _ => BridgeError::BufferOverflow,
                })?,
                Either::Right(_) => return Err(BridgeError::Timeout),
            }
        } else {
            self.tcp.listen().await.map_err(|e| match e {
                ModbusError::PayloadTooShort => BridgeError::TcpClosed,
                ModbusError::Tcp(te) => BridgeError::TcpIo(te),
                _ => BridgeError::BufferOverflow,
            })?
        };

        let (unit_id, fc_byte, start_address, register_count) =
            frame::parse_tcp_request(&tcp_req).unwrap_or((0, 0, 0, 0));

        let (rtu_req, tid) =
            frame::tcp_to_rtu(&tcp_req).map_err(|_| BridgeError::BufferOverflow)?;

        let rtu_resp = if let Some(ms) = self.bridge.rtu_timeout_ms {
            let rtu = &mut self.bridge.rtu;
            let delay = &mut self.bridge.delay;
            let rtu_fut = pin!(rtu.send_and_receive(&rtu_req));
            let delay_fut = pin!(delay.delay_ms(ms));
            match select(rtu_fut, delay_fut).await {
                Either::Left((r, _)) => r.map_err(|e| match e {
                    ModbusError::Serial(se) => BridgeError::RtuIo(se),
                    ModbusError::Crc => BridgeError::RtuCrcMismatch,
                    ModbusError::PayloadTooShort => BridgeError::RtuCrcMismatch,
                    _ => BridgeError::BufferOverflow,
                })?,
                Either::Right(_) => return Err(BridgeError::Timeout),
            }
        } else {
            self.bridge.rtu.send_and_receive(&rtu_req).await
                .map_err(|e| match e {
                    ModbusError::Serial(se) => BridgeError::RtuIo(se),
                    ModbusError::Crc => BridgeError::RtuCrcMismatch,
                    ModbusError::PayloadTooShort => BridgeError::RtuCrcMismatch,
                    _ => BridgeError::BufferOverflow,
                })?
        };

        let (tcp_resp, tid_warning) = match frame::rtu_resp_to_tcp(&rtu_resp, tid) {
            Ok(r) => (r, None),
            Err(ModbusError::InvalidTransactionId) => {
                let fallback = frame::rtu_resp_to_tcp(&rtu_resp, 0)
                    .map_err(|_| BridgeError::BufferOverflow)?;
                let rx_tid = rtu_resp.get(..2)
                    .map(|b| u16::from_be_bytes([b[0], b[1]]))
                    .unwrap_or(0);
                (fallback, Some(Warning::TransactionIdMismatch { expected: tid, got: rx_tid }))
            }
            Err(_) => return Err(BridgeError::BufferOverflow),
        };

        self.tcp.send(&tcp_resp).await.map_err(BridgeError::TcpIo)?;

        if let Some(w) = tid_warning {
            return Ok(BridgeEvent::Warning(w));
        }
        Ok(BridgeEvent::Transaction(Transaction {
            unit_id,
            function_code: FunctionCode::from(fc_byte),
            start_address,
            register_count,
        }))
    }
}

// ── Sync next() — no timeout ──────────────────────────────────────────────────

#[cfg(feature = "sync")]
impl<S, TX, TS> Connection<'_, S, TX, TS, NoDelay>
where
    S: embedded_io::Read + embedded_io::Write,
    TX: OutputPin,
    TS: embedded_io::Read + embedded_io::Write,
{
    /// Drives one complete Modbus request/response cycle (blocking).
    pub fn next(&mut self) -> Result<BridgeEvent, BridgeError<S::Error, TS::Error>> {
        let tcp_req = self.tcp.listen().map_err(|e| match e {
            ModbusError::PayloadTooShort => BridgeError::TcpClosed,
            ModbusError::Tcp(te) => BridgeError::TcpIo(te),
            ModbusError::Push => BridgeError::BufferOverflow,
            _ => BridgeError::BufferOverflow,
        })?;

        let (unit_id, fc_byte, start_address, register_count) =
            frame::parse_tcp_request(&tcp_req).unwrap_or((0, 0, 0, 0));

        let (rtu_req, tid) =
            frame::tcp_to_rtu(&tcp_req).map_err(|_| BridgeError::BufferOverflow)?;

        let rtu_resp = self.bridge.rtu.send_and_receive(&rtu_req)
            .map_err(|e| match e {
                ModbusError::Serial(se) => BridgeError::RtuIo(se),
                ModbusError::Crc => BridgeError::RtuCrcMismatch,
                ModbusError::PayloadTooShort => BridgeError::RtuCrcMismatch,
                _ => BridgeError::BufferOverflow,
            })?;

        let (tcp_resp, tid_warning) = match frame::rtu_resp_to_tcp(&rtu_resp, tid) {
            Ok(r) => (r, None),
            Err(ModbusError::InvalidTransactionId) => {
                let fallback = frame::rtu_resp_to_tcp(&rtu_resp, 0)
                    .map_err(|_| BridgeError::BufferOverflow)?;
                let rx_tid = if rtu_resp.len() >= 2 {
                    u16::from_be_bytes([rtu_resp[0], rtu_resp[1]])
                } else { 0 };
                (fallback, Some(Warning::TransactionIdMismatch { expected: tid, got: rx_tid }))
            }
            Err(_) => return Err(BridgeError::BufferOverflow),
        };

        self.tcp.send(&tcp_resp).map_err(BridgeError::TcpIo)?;

        if let Some(w) = tid_warning {
            return Ok(BridgeEvent::Warning(w));
        }
        Ok(BridgeEvent::Transaction(Transaction {
            unit_id,
            function_code: FunctionCode::from(fc_byte),
            start_address,
            register_count,
        }))
    }
}

// ── Sync next() — with timeout ────────────────────────────────────────────────

#[cfg(feature = "sync")]
impl<S, TX, TS, D> Connection<'_, S, TX, TS, D>
where
    S: embedded_io::Read + embedded_io::Write + embedded_io::ReadReady,
    TX: OutputPin,
    TS: embedded_io::Read + embedded_io::Write + embedded_io::ReadReady,
    D: embedded_hal::delay::DelayNs,
{
    /// Drives one complete Modbus request/response cycle (blocking) with timeout support.
    ///
    /// Polls `ReadReady` before each I/O operation to enforce the timeout budget.
    pub fn next(&mut self) -> Result<BridgeEvent, BridgeError<S::Error, TS::Error>> {
        if let Some(timeout_ms) = self.bridge.tcp_timeout_ms {
            let mut elapsed = 0u32;
            loop {
                match self.tcp.stream.read_ready() {
                    Ok(true) => break,
                    Ok(false) => {
                        if elapsed >= timeout_ms {
                            return Err(BridgeError::Timeout);
                        }
                        self.bridge.delay.delay_ms(1);
                        elapsed = elapsed.saturating_add(1);
                    }
                    Err(e) => return Err(BridgeError::TcpIo(e)),
                }
            }
        }

        let tcp_req = self.tcp.listen().map_err(|e| match e {
            ModbusError::PayloadTooShort => BridgeError::TcpClosed,
            ModbusError::Tcp(te) => BridgeError::TcpIo(te),
            ModbusError::Push => BridgeError::BufferOverflow,
            _ => BridgeError::BufferOverflow,
        })?;

        let (unit_id, fc_byte, start_address, register_count) =
            frame::parse_tcp_request(&tcp_req).unwrap_or((0, 0, 0, 0));

        let (rtu_req, tid) =
            frame::tcp_to_rtu(&tcp_req).map_err(|_| BridgeError::BufferOverflow)?;

        if let Some(timeout_ms) = self.bridge.rtu_timeout_ms {
            let mut elapsed = 0u32;
            loop {
                match self.bridge.rtu.serial.read_ready() {
                    Ok(true) => break,
                    Ok(false) => {
                        if elapsed >= timeout_ms {
                            return Err(BridgeError::Timeout);
                        }
                        self.bridge.delay.delay_ms(1);
                        elapsed = elapsed.saturating_add(1);
                    }
                    Err(e) => return Err(BridgeError::RtuIo(e)),
                }
            }
        }

        let rtu_resp = self.bridge.rtu.send_and_receive(&rtu_req)
            .map_err(|e| match e {
                ModbusError::Serial(se) => BridgeError::RtuIo(se),
                ModbusError::Crc => BridgeError::RtuCrcMismatch,
                ModbusError::PayloadTooShort => BridgeError::RtuCrcMismatch,
                _ => BridgeError::BufferOverflow,
            })?;

        let (tcp_resp, tid_warning) = match frame::rtu_resp_to_tcp(&rtu_resp, tid) {
            Ok(r) => (r, None),
            Err(ModbusError::InvalidTransactionId) => {
                let fallback = frame::rtu_resp_to_tcp(&rtu_resp, 0)
                    .map_err(|_| BridgeError::BufferOverflow)?;
                let rx_tid = if rtu_resp.len() >= 2 {
                    u16::from_be_bytes([rtu_resp[0], rtu_resp[1]])
                } else { 0 };
                (fallback, Some(Warning::TransactionIdMismatch { expected: tid, got: rx_tid }))
            }
            Err(_) => return Err(BridgeError::BufferOverflow),
        };

        self.tcp.send(&tcp_resp).map_err(BridgeError::TcpIo)?;

        if let Some(w) = tid_warning {
            return Ok(BridgeEvent::Warning(w));
        }
        Ok(BridgeEvent::Transaction(Transaction {
            unit_id,
            function_code: FunctionCode::from(fc_byte),
            start_address,
            register_count,
        }))
    }
}
