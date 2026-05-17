//! [`Client`] — owns the RTU serial port and creates [`ClientSession`]s.
//!
//! In client mode the roles of RTU and TCP are reversed relative to [`Bridge`](crate::Bridge):
//! the serial bus acts as the *request source* (an RTU master talks to this device) and
//! the TCP stream connects to an *upstream Modbus TCP server*.

use crate::{
    client_builder::ClientBuilder, client_session::ClientSession, rtu::ModbusRtu, NoDelay,
};
use embedded_hal::digital::OutputPin;

/// Modbus RTU→TCP client.
///
/// Owns the serial port (`S`) and RS-485 TX-enable pin (`TX`). Connect to an
/// upstream Modbus TCP server by calling [`connect`](Client::connect) with a TCP stream.
///
/// The optional third parameter `D` is a delay provider for I/O timeouts.
/// It defaults to [`NoDelay`](crate::NoDelay).
///
/// # Examples
///
/// ```rust,ignore
/// use modbus_bridge::{Client, BridgeError, BridgeEvent};
///
/// let mut client = Client::builder()
///     .rtu(uart, tx_en_pin)
///     .build();
///
/// // tcp_stream connects to the upstream Modbus TCP server
/// let mut session = client.connect(tcp_stream);
/// loop {
///     match session.next().await {
///         Ok(BridgeEvent::Transaction(t)) => log::info!("modbus: {t}"),
///         Ok(BridgeEvent::Warning(w))     => log::warn!("modbus: {w}"),
///         Err(BridgeError::RtuClosed)     => break,  // RTU master disconnected
///         Err(e)                          => { log::error!("{e}"); break; }
///     }
/// }
/// let tcp_stream = session.into_stream();
/// ```
pub struct Client<S, TX, D = NoDelay> {
    pub(crate) rtu: ModbusRtu<S, TX>,
    pub(crate) rtu_timeout_ms: Option<u32>,
    pub(crate) tcp_timeout_ms: Option<u32>,
    pub(crate) delay: D,
}

impl<S, TX, D> Client<S, TX, D> {
    /// Returns a [`ClientBuilder`](crate::ClientBuilder) for constructing a `Client`.
    pub fn builder() -> ClientBuilder<(), (), NoDelay> {
        ClientBuilder::new()
    }

    pub(crate) fn from_parts(
        serial: S,
        tx_en: TX,
        delay: D,
        rtu_timeout_ms: Option<u32>,
        tcp_timeout_ms: Option<u32>,
    ) -> Self {
        Self {
            rtu: ModbusRtu::new(serial, tx_en),
            rtu_timeout_ms,
            tcp_timeout_ms,
            delay,
        }
    }

    /// Consumes the client and returns the inner serial port, TX-enable pin, and delay provider.
    pub fn into_inner(self) -> (S, TX, D) {
        let (s, tx) = self.rtu.into_inner();
        (s, tx, self.delay)
    }
}

#[cfg(feature = "async")]
impl<S, TX, D> Client<S, TX, D>
where
    S: embedded_io_async::Read + embedded_io_async::Write,
    TX: OutputPin,
{
    /// Creates a [`ClientSession`](crate::ClientSession) connected to an upstream TCP server.
    ///
    /// Takes ownership of `stream` and mutably borrows the client for the lifetime
    /// of the returned [`ClientSession`](crate::ClientSession).
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let mut session = client.connect(tcp_stream);
    /// loop {
    ///     match session.next().await {
    ///         Ok(event) => { /* handle event */ }
    ///         Err(_) => break,
    ///     }
    /// }
    /// let tcp_stream = session.into_stream();
    /// ```
    pub fn connect<TS>(&mut self, stream: TS) -> ClientSession<'_, S, TX, TS, D>
    where
        TS: embedded_io_async::Read + embedded_io_async::Write,
    {
        ClientSession::new(self, stream)
    }
}

#[cfg(feature = "sync")]
impl<S, TX, D> Client<S, TX, D>
where
    S: embedded_io::Read + embedded_io::Write,
    TX: OutputPin,
{
    /// Creates a [`ClientSession`](crate::ClientSession) connected to an upstream TCP server.
    pub fn connect<TS>(&mut self, stream: TS) -> ClientSession<'_, S, TX, TS, D>
    where
        TS: embedded_io::Read + embedded_io::Write,
    {
        ClientSession::new(self, stream)
    }
}
