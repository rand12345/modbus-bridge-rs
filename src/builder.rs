//! Typestate builder for [`Bridge`](crate::Bridge).

use crate::{bridge::Bridge, NoDelay, NoPin};

/// Builder for [`Bridge`]. Obtain via [`Bridge::builder()`](crate::Bridge::builder).
///
/// Uses a typestate pattern: `S` and `TX` start as `()` and are replaced when
/// `.rtu()` is called, preventing `.build()` before the serial port is supplied.
/// `D` starts as [`NoDelay`](crate::NoDelay) and can be upgraded with `.delay()`.
pub struct BridgeBuilder<S, TX, D = NoDelay> {
    pub(crate) serial: S,
    pub(crate) tx_en: TX,
    pub(crate) rtu_timeout_ms: Option<u32>,
    pub(crate) tcp_timeout_ms: Option<u32>,
    pub(crate) delay: D,
}

impl BridgeBuilder<(), (), NoDelay> {
    /// Creates a new `BridgeBuilder` with no serial port configured.
    ///
    /// Prefer [`Bridge::builder()`](crate::Bridge::builder) over calling this directly.
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

impl Default for BridgeBuilder<(), (), NoDelay> {
    fn default() -> Self {
        Self::new()
    }
}

impl<D> BridgeBuilder<(), (), D> {
    /// Supplies the serial port and RS-485 TX-enable pin.
    pub fn rtu<S, TX>(self, serial: S, tx_en: TX) -> BridgeBuilder<S, TX, D> {
        BridgeBuilder {
            serial,
            tx_en,
            rtu_timeout_ms: self.rtu_timeout_ms,
            tcp_timeout_ms: self.tcp_timeout_ms,
            delay: self.delay,
        }
    }

    /// Supplies the serial port without a TX-enable pin.
    pub fn rtu_no_pin<S>(self, serial: S) -> BridgeBuilder<S, NoPin, D> {
        BridgeBuilder {
            serial,
            tx_en: NoPin,
            rtu_timeout_ms: self.rtu_timeout_ms,
            tcp_timeout_ms: self.tcp_timeout_ms,
            delay: self.delay,
        }
    }
}

impl<S, TX, D> BridgeBuilder<S, TX, D> {
    /// Sets the RTU I/O timeout in milliseconds.
    ///
    /// Applied while waiting for the RTU device response in each cycle.
    /// Requires a delay provider — call `.delay()` as well.
    pub fn rtu_timeout(mut self, ms: u32) -> Self {
        self.rtu_timeout_ms = Some(ms);
        self
    }

    /// Sets the TCP I/O timeout in milliseconds.
    ///
    /// Applied while waiting for an incoming TCP request in each cycle.
    /// Requires a delay provider — call `.delay()` as well.
    pub fn tcp_timeout(mut self, ms: u32) -> Self {
        self.tcp_timeout_ms = Some(ms);
        self
    }

    /// Builds and returns the configured [`Bridge`](crate::Bridge).
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let mut bridge = Bridge::builder()
    ///     .rtu(uart, tx_en)
    ///     .build();
    /// ```
    pub fn build(self) -> Bridge<S, TX, D> {
        Bridge::from_parts(
            self.serial,
            self.tx_en,
            self.delay,
            self.rtu_timeout_ms,
            self.tcp_timeout_ms,
        )
    }
}

impl<S, TX> BridgeBuilder<S, TX, NoDelay> {
    /// Supplies a delay provider and upgrades `D` from `NoDelay`.
    ///
    /// Must be called before `.build()` when using `.rtu_timeout()` or `.tcp_timeout()`.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let bridge = Bridge::builder()
    ///     .rtu(uart, pin)
    ///     .rtu_timeout(500)
    ///     .delay(my_timer)
    ///     .build();
    /// ```
    pub fn delay<D2>(self, delay: D2) -> BridgeBuilder<S, TX, D2> {
        BridgeBuilder {
            serial: self.serial,
            tx_en: self.tx_en,
            rtu_timeout_ms: self.rtu_timeout_ms,
            tcp_timeout_ms: self.tcp_timeout_ms,
            delay,
        }
    }
}
