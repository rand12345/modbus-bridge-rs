//! The [`Bridge`] type — owns the RTU serial port and creates [`Connection`]s.

use crate::{builder::BridgeBuilder, connection::Connection, rtu::ModbusRtu, NoDelay};
use embedded_hal::digital::OutputPin;

/// Modbus RTU/TCP bridge.
///
/// Owns the serial port (`S`) and RS-485 TX-enable pin (`TX`). TCP connections
/// are supplied one at a time via [`accept`](Bridge::accept). Only one
/// [`Connection`](crate::Connection) can be active at a time — the bridge is
/// mutably borrowed for the connection's lifetime.
///
/// The optional third parameter `D` is a delay provider for I/O timeouts.
/// It defaults to [`NoDelay`](crate::NoDelay); configure it via
/// [`BridgeBuilder::delay`](crate::BridgeBuilder::delay).
///
/// # Examples
///
/// ```rust,ignore
/// use modbus_bridge::{Bridge, BridgeError, BridgeEvent};
///
/// let mut bridge = Bridge::builder()
///     .rtu(uart, tx_en_pin)
///     .build();
///
/// loop {
///     let socket = tcp_stack.listen(502).await.unwrap();
///     let mut conn = bridge.accept(socket);
///     loop {
///         match conn.next().await {
///             Ok(BridgeEvent::Transaction(t)) => log::info!("modbus: {t}"),
///             Ok(BridgeEvent::Warning(w))     => log::warn!("modbus: {w}"),
///             Err(BridgeError::TcpClosed)     => break,
///             Err(e)                          => { log::error!("{e}"); break; }
///         }
///     }
///     conn.into_stream().close();
/// }
/// ```
pub struct Bridge<S, TX, D = NoDelay> {
    pub(crate) rtu: ModbusRtu<S, TX>,
    pub(crate) rtu_timeout_ms: Option<u32>,
    pub(crate) tcp_timeout_ms: Option<u32>,
    pub(crate) delay: D,
}

impl<S, TX, D> Bridge<S, TX, D> {
    /// Returns a [`BridgeBuilder`](crate::BridgeBuilder) for constructing a `Bridge`.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// use modbus_bridge::Bridge;
    ///
    /// let bridge = Bridge::builder()
    ///     .rtu(uart, tx_en)
    ///     .build();
    /// ```
    pub fn builder() -> BridgeBuilder<(), (), NoDelay> {
        BridgeBuilder::new()
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

    /// Consumes the bridge and returns the inner serial port, TX-enable pin, and delay provider.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let (uart, tx_en, _delay) = bridge.into_inner();
    /// ```
    pub fn into_inner(self) -> (S, TX, D) {
        let (s, tx) = self.rtu.into_inner();
        (s, tx, self.delay)
    }
}

#[cfg(feature = "async")]
impl<S, TX, D> Bridge<S, TX, D>
where
    S: embedded_io_async::Read + embedded_io_async::Write,
    TX: OutputPin,
{
    /// Creates a [`Connection`](crate::Connection) for an incoming TCP client.
    ///
    /// Takes ownership of `stream` and mutably borrows the bridge for the
    /// lifetime of the returned [`Connection`](crate::Connection).
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let mut conn = bridge.accept(socket);
    /// loop {
    ///     match conn.next().await {
    ///         Ok(event) => { /* handle event */ }
    ///         Err(_)    => break,
    ///     }
    /// }
    /// let socket = conn.into_stream();
    /// socket.close();
    /// ```
    pub fn accept<TS>(&mut self, stream: TS) -> Connection<'_, S, TX, TS, D>
    where
        TS: embedded_io_async::Read + embedded_io_async::Write,
    {
        Connection::new(self, stream)
    }
}

#[cfg(feature = "sync")]
impl<S, TX, D> Bridge<S, TX, D>
where
    S: embedded_io::Read + embedded_io::Write,
    TX: OutputPin,
{
    /// Creates a [`Connection`](crate::Connection) for an incoming TCP client.
    pub fn accept<TS>(&mut self, stream: TS) -> Connection<'_, S, TX, TS, D>
    where
        TS: embedded_io::Read + embedded_io::Write,
    {
        Connection::new(self, stream)
    }
}
