//! Public event and error types for the [`Bridge`](crate::Bridge) API.

use core::fmt;

// ── Function code ─────────────────────────────────────────────────────────────

/// Modbus function code extracted from a request frame.
///
/// Covers the most common read/write operations. Unknown function codes are
/// wrapped in [`Other`](FunctionCode::Other) and forwarded to the RTU device
/// without modification, so vendor-specific extensions work transparently.
///
/// # Examples
///
/// ```rust,ignore
/// if let BridgeEvent::Transaction(t) = event {
///     match t.function_code {
///         FunctionCode::ReadHoldingRegisters => { /* … */ }
///         FunctionCode::WriteMultipleRegisters => { /* … */ }
///         other => defmt::warn!("unexpected FC: {}", other),
///     }
/// }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum FunctionCode {
    /// FC 0x01 — reads one or more output coils (digital outputs).
    ReadCoils,
    /// FC 0x02 — reads one or more discrete inputs (digital inputs).
    ReadDiscreteInputs,
    /// FC 0x03 — reads one or more holding registers (read/write 16-bit words).
    ReadHoldingRegisters,
    /// FC 0x04 — reads one or more input registers (read-only 16-bit words).
    ReadInputRegisters,
    /// FC 0x05 — writes a single output coil.
    WriteSingleCoil,
    /// FC 0x06 — writes a single holding register.
    WriteSingleRegister,
    /// FC 0x0F — writes multiple output coils in a single request.
    WriteMultipleCoils,
    /// FC 0x10 — writes multiple holding registers in a single request.
    WriteMultipleRegisters,
    /// Any function code not listed above — passed through to the RTU device transparently.
    Other(u8),
}

impl From<u8> for FunctionCode {
    fn from(v: u8) -> Self {
        match v {
            0x01 => Self::ReadCoils,
            0x02 => Self::ReadDiscreteInputs,
            0x03 => Self::ReadHoldingRegisters,
            0x04 => Self::ReadInputRegisters,
            0x05 => Self::WriteSingleCoil,
            0x06 => Self::WriteSingleRegister,
            0x0F => Self::WriteMultipleCoils,
            0x10 => Self::WriteMultipleRegisters,
            other => Self::Other(other),
        }
    }
}

impl fmt::Display for FunctionCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ReadCoils => write!(f, "ReadCoils"),
            Self::ReadDiscreteInputs => write!(f, "ReadDiscreteInputs"),
            Self::ReadHoldingRegisters => write!(f, "ReadHoldingRegisters"),
            Self::ReadInputRegisters => write!(f, "ReadInputRegisters"),
            Self::WriteSingleCoil => write!(f, "WriteSingleCoil"),
            Self::WriteSingleRegister => write!(f, "WriteSingleRegister"),
            Self::WriteMultipleCoils => write!(f, "WriteMultipleCoils"),
            Self::WriteMultipleRegisters => write!(f, "WriteMultipleRegisters"),
            Self::Other(n) => write!(f, "FC({:#04x})", n),
        }
    }
}

// ── Transaction ───────────────────────────────────────────────────────────────

/// A successfully completed Modbus request/response cycle.
///
/// Returned inside [`BridgeEvent::Transaction`] after
/// [`Connection::next`](crate::Connection::next) successfully forwards a request
/// to the RTU device and relays the response back to the TCP client.
///
/// # Examples
///
/// ```rust,ignore
/// if let BridgeEvent::Transaction(t) = conn.next().await? {
///     defmt::info!(
///         "unit={} fc={} addr={} count={}",
///         t.unit_id, t.function_code, t.start_address, t.register_count,
///     );
/// }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Transaction {
    /// Modbus unit (slave) address from the request, in the range 1–247.
    pub unit_id: u8,
    /// Function code identifying the type of operation requested.
    pub function_code: FunctionCode,
    /// Starting register or coil address (zero-based) from the request.
    pub start_address: u16,
    /// Number of registers or coils requested, or the output value for
    /// single-write function codes (FC 0x05, FC 0x06).
    pub register_count: u16,
}

impl fmt::Display for Transaction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "unit={} fc={} addr={} count={}",
            self.unit_id, self.function_code, self.start_address, self.register_count
        )
    }
}

// ── Warning ───────────────────────────────────────────────────────────────────

/// Non-fatal protocol anomaly detected during a request/response cycle.
///
/// Returned inside [`BridgeEvent::Warning`] by
/// [`Connection::next`](crate::Connection::next). The connection remains open
/// after a warning — the response was still forwarded to the TCP client.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Warning {
    /// The transaction ID in the RTU response did not match the one sent in the
    /// request. The response was forwarded using transaction ID 0 as a fallback.
    ///
    /// This can occur with RTU devices that echo back stale or incorrect
    /// transaction IDs. It is safe to continue after this warning.
    TransactionIdMismatch { expected: u16, got: u16 },
}

impl fmt::Display for Warning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TransactionIdMismatch { expected, got } => write!(
                f,
                "transaction ID mismatch: expected {expected}, got {got}"
            ),
        }
    }
}

// ── BridgeEvent ───────────────────────────────────────────────────────────────

/// Successful outcome returned by [`Connection::next`](crate::Connection::next).
///
/// Inspect the variant to decide how to log or react:
///
/// - [`Transaction`](BridgeEvent::Transaction) — normal operation; one full
///   request/response cycle completed.
/// - [`Warning`](BridgeEvent::Warning) — a non-fatal anomaly was detected and
///   the connection is still running. Log the warning and keep calling `next`.
///
/// # Examples
///
/// ```rust,ignore
/// match conn.next().await? {
///     BridgeEvent::Transaction(t) => defmt::info!("ok: {}", t),
///     BridgeEvent::Warning(w)     => defmt::warn!("warn: {}", w),
/// }
/// ```
#[derive(Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum BridgeEvent {
    /// One complete Modbus request/response cycle completed successfully.
    Transaction(Transaction),
    /// A non-fatal protocol anomaly was detected; the connection continues.
    ///
    /// See [`Warning`] for details on individual anomaly kinds.
    Warning(Warning),
}

impl fmt::Display for BridgeEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Transaction(t) => fmt::Display::fmt(t, f),
            Self::Warning(w) => fmt::Display::fmt(w, f),
        }
    }
}

// ── BridgeError ───────────────────────────────────────────────────────────────

/// Hard error returned by [`Connection::next`](crate::Connection::next).
///
/// On any `BridgeError` the caller should exit the connection loop, close the
/// TCP stream, and call [`Bridge::accept`](crate::Bridge::accept) for the next
/// client:
///
/// ```rust,ignore
/// loop {
///     match conn.next().await {
///         Ok(event) => { /* handle */ }
///         Err(BridgeError::TcpClosed) => break,  // normal disconnect
///         Err(e) => { defmt::error!("{}", e); break; }  // hard error
///     }
/// }
/// conn.into_stream().close();
/// ```
///
/// `SE` is the serial-port error type and `TE` is the TCP-stream error type.
/// Both come from the [`embedded_io_async::ErrorType`] (or [`embedded_io::ErrorType`])
/// implementations of the serial and TCP types passed to
/// [`BridgeBuilder`](crate::BridgeBuilder).
#[derive(Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum BridgeError<SE, TE> {
    /// TCP client closed the connection cleanly (EOF / zero-byte read).
    ///
    /// This is the normal exit condition and is not an error in itself. Break
    /// the connection loop and accept the next client.
    TcpClosed,
    /// RTU master closed the serial connection cleanly (EOF / zero-byte read).
    ///
    /// Normal exit condition in client mode — equivalent to `TcpClosed` in bridge mode.
    RtuClosed,
    /// TCP I/O error from the underlying stream.
    TcpIo(TE),
    /// Serial (RTU) I/O error from the underlying serial port.
    RtuIo(SE),
    /// RTU device response failed CRC-16 verification.
    ///
    /// This usually indicates a wiring problem, an incorrect baud rate, or
    /// electrical noise on the RS-485 bus.
    RtuCrcMismatch,
    /// A Modbus frame was larger than the internal frame buffer can hold.
    ///
    /// The internal buffers support the full Modbus specification maximum
    /// (255-byte RTU / 261-byte TCP). This error indicates a malformed or
    /// non-Modbus frame.
    BufferOverflow,
    /// An RTU or TCP I/O operation did not complete within the configured timeout.
    Timeout,
}

impl<SE: fmt::Debug, TE: fmt::Debug> fmt::Display for BridgeError<SE, TE> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TcpClosed => write!(f, "TCP connection closed"),
            Self::RtuClosed => write!(f, "RTU connection closed"),
            Self::TcpIo(e) => write!(f, "TCP I/O error: {:?}", e),
            Self::RtuIo(e) => write!(f, "RTU I/O error: {:?}", e),
            Self::RtuCrcMismatch => write!(f, "RTU response CRC mismatch"),
            Self::BufferOverflow => write!(f, "frame buffer overflow — increase BUF capacity"),
            Self::Timeout => write!(f, "I/O timeout"),
        }
    }
}
