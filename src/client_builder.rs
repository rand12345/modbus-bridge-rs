//! Typestate builder for [`Client`](crate::Client).

use crate::{client::Client, NoDelay, NoPin};

/// Builder for [`Client`]. Obtain via [`Client::builder()`](crate::Client::builder).
///
/// Uses the same typestate pattern as [`BridgeBuilder`](crate::BridgeBuilder).
/// `S` and `TX` start as `()` until `.rtu()` is called. `D` starts as
/// [`NoDelay`](crate::NoDelay) and can be upgraded with `.delay()`.
pub struct ClientBuilder<S, TX, D = NoDelay> {
    pub(crate) serial: S,
    pub(crate) tx_en: TX,
    pub(crate) rtu_timeout_ms: Option<u32>,
    pub(crate) tcp_timeout_ms: Option<u32>,
    pub(crate) delay: D,
}

impl ClientBuilder<(), (), NoDelay> {
    /// Creates a new `ClientBuilder` with no serial port configured.
    ///
    /// Prefer [`Client::builder()`](crate::Client::builder) over calling this directly.
    pub fn new() -> Self {
        Self {
            serial: (),
            tx_en: (),
            rtu_timeout_ms: None,
            tcp_timeout_ms: None,
            delay: NoDelay,
        }
    }
}

impl Default for ClientBuilder<(), (), NoDelay> {
    fn default() -> Self {
        Self::new()
    }
}

impl<D> ClientBuilder<(), (), D> {
    /// Supplies the serial port and RS-485 TX-enable pin.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let client = Client::builder()
    ///     .rtu(uart, tx_en_pin)
    ///     .build();
    /// ```
    pub fn rtu<S, TX>(self, serial: S, tx_en: TX) -> ClientBuilder<S, TX, D> {
        ClientBuilder {
            serial,
            tx_en,
            rtu_timeout_ms: self.rtu_timeout_ms,
            tcp_timeout_ms: self.tcp_timeout_ms,
            delay: self.delay,
        }
    }

    /// Supplies the serial port without a TX-enable pin.
    ///
    /// Use this when the RS-485 transceiver handles direction control automatically.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let client = Client::builder()
    ///     .rtu_no_pin(uart)
    ///     .build();
    /// ```
    pub fn rtu_no_pin<S>(self, serial: S) -> ClientBuilder<S, NoPin, D> {
        ClientBuilder {
            serial,
            tx_en: NoPin,
            rtu_timeout_ms: self.rtu_timeout_ms,
            tcp_timeout_ms: self.tcp_timeout_ms,
            delay: self.delay,
        }
    }
}

impl<S, TX, D> ClientBuilder<S, TX, D> {
    /// Sets the RTU I/O timeout in milliseconds.
    ///
    /// Applied while waiting for an incoming RTU request from the serial master.
    /// Requires a delay provider — call `.delay()` as well.
    pub fn rtu_timeout(mut self, ms: u32) -> Self {
        self.rtu_timeout_ms = Some(ms);
        self
    }

    /// Sets the TCP I/O timeout in milliseconds.
    ///
    /// Applied while waiting for the upstream TCP server response.
    /// Requires a delay provider — call `.delay()` as well.
    pub fn tcp_timeout(mut self, ms: u32) -> Self {
        self.tcp_timeout_ms = Some(ms);
        self
    }

    /// Builds and returns the configured [`Client`](crate::Client).
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let mut client = Client::builder()
    ///     .rtu(uart, tx_en)
    ///     .build();
    /// ```
    pub fn build(self) -> Client<S, TX, D> {
        Client::from_parts(
            self.serial,
            self.tx_en,
            self.delay,
            self.rtu_timeout_ms,
            self.tcp_timeout_ms,
        )
    }
}

impl<S, TX> ClientBuilder<S, TX, NoDelay> {
    /// Supplies a delay provider and upgrades `D` from `NoDelay`.
    ///
    /// Must be called before `.build()` when using `.rtu_timeout()` or `.tcp_timeout()`.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let client = Client::builder()
    ///     .rtu(uart, pin)
    ///     .rtu_timeout(500)
    ///     .delay(my_timer)
    ///     .build();
    /// ```
    pub fn delay<D2>(self, delay: D2) -> ClientBuilder<S, TX, D2> {
        ClientBuilder {
            serial: self.serial,
            tx_en: self.tx_en,
            rtu_timeout_ms: self.rtu_timeout_ms,
            tcp_timeout_ms: self.tcp_timeout_ms,
            delay,
        }
    }
}
