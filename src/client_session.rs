//! [`ClientSession`] — a live TCP session bound to a [`Client`](crate::Client).

use crate::{
    client::Client,
    error::ModbusError,
    event::{BridgeError, BridgeEvent, FunctionCode, Transaction, Warning},
    frame,
    tcp::ModbusTcp,
    NoDelay,
};
use embedded_hal::digital::OutputPin;

/// An active Modbus RTU → TCP client session.
///
/// Returned by [`Client::connect`](crate::Client::connect). Mutably borrows the
/// client for its lifetime, preventing a second session from being opened until
/// this one is finished.
///
/// Drive the session by calling [`next`](ClientSession::next) in a loop.
pub struct ClientSession<'b, S, TX, TS, D = NoDelay> {
    pub(crate) client: &'b mut Client<S, TX, D>,
    pub(crate) tcp: ModbusTcp<TS>,
}

impl<'b, S, TX, TS, D> ClientSession<'b, S, TX, TS, D> {
    pub(crate) fn new(client: &'b mut Client<S, TX, D>, stream: TS) -> Self {
        Self { client, tcp: ModbusTcp::new(stream) }
    }

    /// Consumes the session and returns the underlying TCP stream.
    pub fn into_stream(self) -> TS {
        self.tcp.into_inner()
    }
}

// ── Async next() — no timeout ─────────────────────────────────────────────────

#[cfg(feature = "async")]
impl<S, TX, TS> ClientSession<'_, S, TX, TS, NoDelay>
where
    S: embedded_io_async::Read + embedded_io_async::Write,
    TX: OutputPin,
    TS: embedded_io_async::Read + embedded_io_async::Write,
{
    /// Drives one complete Modbus request/response cycle asynchronously.
    ///
    /// Reads an RTU request from the serial port, forwards it to the upstream
    /// Modbus TCP server, and returns the response to the RTU master.
    ///
    /// # Errors
    ///
    /// Returns [`BridgeError::RtuClosed`](crate::BridgeError::RtuClosed) when the
    /// RTU master closes the connection cleanly.
    pub async fn next(&mut self) -> Result<BridgeEvent, BridgeError<S::Error, TS::Error>> {
        let rtu_req = self.client.rtu.listen().await.map_err(|e| match e {
            ModbusError::Serial(se) => BridgeError::RtuIo(se),
            ModbusError::Crc => BridgeError::RtuCrcMismatch,
            ModbusError::PayloadTooShort => BridgeError::RtuClosed,
            _ => BridgeError::BufferOverflow,
        })?;

        let (unit_id, fc_byte, start_address, register_count) =
            frame::parse_rtu_request(&rtu_req).unwrap_or((0, 0, 0, 0));

        let tcp_req = frame::rtu_to_tcp(&rtu_req, self.tcp.next_transaction_id())
            .map_err(|_| BridgeError::BufferOverflow)?;

        let tid = u16::from_be_bytes([tcp_req[0], tcp_req[1]]);

        self.tcp.send(&tcp_req).await.map_err(BridgeError::TcpIo)?;

        let tcp_resp = self.tcp.listen().await.map_err(|e| match e {
            ModbusError::PayloadTooShort => BridgeError::TcpClosed,
            ModbusError::Tcp(te) => BridgeError::TcpIo(te),
            _ => BridgeError::BufferOverflow,
        })?;

        let (rtu_resp, tid_warning) = match frame::tcp_resp_to_rtu(&tcp_resp, tid) {
            Ok(r) => (r, None),
            Err(ModbusError::InvalidTransactionId) => {
                let rx_tid = tcp_resp.get(..2)
                    .map(|b| u16::from_be_bytes([b[0], b[1]]))
                    .unwrap_or(0);
                let fallback = frame::tcp_resp_to_rtu(&tcp_resp, rx_tid)
                    .map_err(|_| BridgeError::BufferOverflow)?;
                (fallback, Some(Warning::TransactionIdMismatch { expected: tid, got: rx_tid }))
            }
            Err(_) => return Err(BridgeError::BufferOverflow),
        };

        self.client.rtu.send(&rtu_resp).await.map_err(BridgeError::RtuIo)?;

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
impl<S, TX, TS, D> ClientSession<'_, S, TX, TS, D>
where
    S: embedded_io_async::Read + embedded_io_async::Write,
    TX: OutputPin,
    TS: embedded_io_async::Read + embedded_io_async::Write,
    D: embedded_hal_async::delay::DelayNs,
{
    /// Drives one complete Modbus request/response cycle asynchronously, with timeouts.
    pub async fn next(&mut self) -> Result<BridgeEvent, BridgeError<S::Error, TS::Error>> {
        use core::pin::pin;
        use futures_util::future::{select, Either};

        let rtu_req = if let Some(ms) = self.client.rtu_timeout_ms {
            let rtu_fut = pin!(self.client.rtu.listen());
            let delay_fut = pin!(self.client.delay.delay_ms(ms));
            match select(rtu_fut, delay_fut).await {
                Either::Left((r, _)) => r.map_err(|e| match e {
                    ModbusError::Serial(se) => BridgeError::RtuIo(se),
                    ModbusError::Crc => BridgeError::RtuCrcMismatch,
                    ModbusError::PayloadTooShort => BridgeError::RtuClosed,
                    _ => BridgeError::BufferOverflow,
                })?,
                Either::Right(_) => return Err(BridgeError::Timeout),
            }
        } else {
            self.client.rtu.listen().await.map_err(|e| match e {
                ModbusError::Serial(se) => BridgeError::RtuIo(se),
                ModbusError::Crc => BridgeError::RtuCrcMismatch,
                ModbusError::PayloadTooShort => BridgeError::RtuClosed,
                _ => BridgeError::BufferOverflow,
            })?
        };

        let (unit_id, fc_byte, start_address, register_count) =
            frame::parse_rtu_request(&rtu_req).unwrap_or((0, 0, 0, 0));

        let tcp_req = frame::rtu_to_tcp(&rtu_req, self.tcp.next_transaction_id())
            .map_err(|_| BridgeError::BufferOverflow)?;

        let tid = u16::from_be_bytes([tcp_req[0], tcp_req[1]]);

        self.tcp.send(&tcp_req).await.map_err(BridgeError::TcpIo)?;

        let tcp_resp = if let Some(ms) = self.client.tcp_timeout_ms {
            let tcp_fut = pin!(self.tcp.listen());
            let delay_fut = pin!(self.client.delay.delay_ms(ms));
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

        let (rtu_resp, tid_warning) = match frame::tcp_resp_to_rtu(&tcp_resp, tid) {
            Ok(r) => (r, None),
            Err(ModbusError::InvalidTransactionId) => {
                let rx_tid = tcp_resp.get(..2)
                    .map(|b| u16::from_be_bytes([b[0], b[1]]))
                    .unwrap_or(0);
                let fallback = frame::tcp_resp_to_rtu(&tcp_resp, rx_tid)
                    .map_err(|_| BridgeError::BufferOverflow)?;
                (fallback, Some(Warning::TransactionIdMismatch { expected: tid, got: rx_tid }))
            }
            Err(_) => return Err(BridgeError::BufferOverflow),
        };

        self.client.rtu.send(&rtu_resp).await.map_err(BridgeError::RtuIo)?;

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
impl<S, TX, TS> ClientSession<'_, S, TX, TS, NoDelay>
where
    S: embedded_io::Read + embedded_io::Write,
    TX: OutputPin,
    TS: embedded_io::Read + embedded_io::Write,
{
    /// Drives one complete Modbus request/response cycle (blocking).
    pub fn next(&mut self) -> Result<BridgeEvent, BridgeError<S::Error, TS::Error>> {
        let rtu_req = self.client.rtu.listen().map_err(|e| match e {
            ModbusError::Serial(se) => BridgeError::RtuIo(se),
            ModbusError::Crc => BridgeError::RtuCrcMismatch,
            ModbusError::PayloadTooShort => BridgeError::RtuClosed,
            _ => BridgeError::BufferOverflow,
        })?;

        let (unit_id, fc_byte, start_address, register_count) =
            frame::parse_rtu_request(&rtu_req).unwrap_or((0, 0, 0, 0));

        let tcp_req = frame::rtu_to_tcp(&rtu_req, self.tcp.next_transaction_id())
            .map_err(|_| BridgeError::BufferOverflow)?;

        let tid = u16::from_be_bytes([tcp_req[0], tcp_req[1]]);

        self.tcp.send(&tcp_req).map_err(BridgeError::TcpIo)?;

        let tcp_resp = self.tcp.listen().map_err(|e| match e {
            ModbusError::PayloadTooShort => BridgeError::TcpClosed,
            ModbusError::Tcp(te) => BridgeError::TcpIo(te),
            ModbusError::Push => BridgeError::BufferOverflow,
            _ => BridgeError::BufferOverflow,
        })?;

        let (rtu_resp, tid_warning) = match frame::tcp_resp_to_rtu(&tcp_resp, tid) {
            Ok(r) => (r, None),
            Err(ModbusError::InvalidTransactionId) => {
                let rx_tid = if tcp_resp.len() >= 2 {
                    u16::from_be_bytes([tcp_resp[0], tcp_resp[1]])
                } else { 0 };
                let fallback = frame::tcp_resp_to_rtu(&tcp_resp, rx_tid)
                    .map_err(|_| BridgeError::BufferOverflow)?;
                (fallback, Some(Warning::TransactionIdMismatch { expected: tid, got: rx_tid }))
            }
            Err(_) => return Err(BridgeError::BufferOverflow),
        };

        self.client.rtu.send(&rtu_resp).map_err(BridgeError::RtuIo)?;

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
impl<S, TX, TS, D> ClientSession<'_, S, TX, TS, D>
where
    S: embedded_io::Read + embedded_io::Write + embedded_io::ReadReady,
    TX: OutputPin,
    TS: embedded_io::Read + embedded_io::Write + embedded_io::ReadReady,
    D: embedded_hal::delay::DelayNs,
{
    /// Drives one complete Modbus request/response cycle (blocking) with timeout support.
    pub fn next(&mut self) -> Result<BridgeEvent, BridgeError<S::Error, TS::Error>> {
        if let Some(timeout_ms) = self.client.rtu_timeout_ms {
            let mut elapsed = 0u32;
            loop {
                match self.client.rtu.serial.read_ready() {
                    Ok(true) => break,
                    Ok(false) => {
                        if elapsed >= timeout_ms {
                            return Err(BridgeError::Timeout);
                        }
                        self.client.delay.delay_ms(1);
                        elapsed = elapsed.saturating_add(1);
                    }
                    Err(e) => return Err(BridgeError::RtuIo(e)),
                }
            }
        }

        let rtu_req = self.client.rtu.listen().map_err(|e| match e {
            ModbusError::Serial(se) => BridgeError::RtuIo(se),
            ModbusError::Crc => BridgeError::RtuCrcMismatch,
            ModbusError::PayloadTooShort => BridgeError::RtuClosed,
            _ => BridgeError::BufferOverflow,
        })?;

        let (unit_id, fc_byte, start_address, register_count) =
            frame::parse_rtu_request(&rtu_req).unwrap_or((0, 0, 0, 0));

        let tcp_req = frame::rtu_to_tcp(&rtu_req, self.tcp.next_transaction_id())
            .map_err(|_| BridgeError::BufferOverflow)?;

        let tid = u16::from_be_bytes([tcp_req[0], tcp_req[1]]);

        self.tcp.send(&tcp_req).map_err(BridgeError::TcpIo)?;

        if let Some(timeout_ms) = self.client.tcp_timeout_ms {
            let mut elapsed = 0u32;
            loop {
                match self.tcp.stream.read_ready() {
                    Ok(true) => break,
                    Ok(false) => {
                        if elapsed >= timeout_ms {
                            return Err(BridgeError::Timeout);
                        }
                        self.client.delay.delay_ms(1);
                        elapsed = elapsed.saturating_add(1);
                    }
                    Err(e) => return Err(BridgeError::TcpIo(e)),
                }
            }
        }

        let tcp_resp = self.tcp.listen().map_err(|e| match e {
            ModbusError::PayloadTooShort => BridgeError::TcpClosed,
            ModbusError::Tcp(te) => BridgeError::TcpIo(te),
            ModbusError::Push => BridgeError::BufferOverflow,
            _ => BridgeError::BufferOverflow,
        })?;

        let (rtu_resp, tid_warning) = match frame::tcp_resp_to_rtu(&tcp_resp, tid) {
            Ok(r) => (r, None),
            Err(ModbusError::InvalidTransactionId) => {
                let rx_tid = if tcp_resp.len() >= 2 {
                    u16::from_be_bytes([tcp_resp[0], tcp_resp[1]])
                } else { 0 };
                let fallback = frame::tcp_resp_to_rtu(&tcp_resp, rx_tid)
                    .map_err(|_| BridgeError::BufferOverflow)?;
                (fallback, Some(Warning::TransactionIdMismatch { expected: tid, got: rx_tid }))
            }
            Err(_) => return Err(BridgeError::BufferOverflow),
        };

        self.client.rtu.send(&rtu_resp).map_err(BridgeError::RtuIo)?;

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
