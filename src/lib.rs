//! Portable `no_std` Modbus RTU/TCP bridge — async and blocking.
//!
//! Accepts Modbus TCP connections and transparently forwards each request to a
//! Modbus RTU device over a serial port, then returns the response to the TCP
//! client. No heap allocation is required: all internal buffers use
//! fixed-capacity [`heapless`] collections.
//!
//! # When to use this crate
//!
//! Use this crate when you need to:
//!
//! - Bridge legacy RS-485/Modbus RTU sensors or PLCs onto a Wi-Fi or Ethernet
//!   network.
//! - Act as a Modbus TCP gateway (port 502) for a home-automation hub, SCADA
//!   system, or any Modbus TCP client.
//! - Run on a microcontroller such as an ESP32, STM32, or RP2040 without an
//!   operating system.
//!
//! # Adding to your project
//!
//! ```toml
//! [dependencies]
//! # Async — Embassy, smoltcp, and other async runtimes (enabled by default)
//! modbus-bridge = { version = "0.3.2", features = ["async", "defmt"] } # x-release-please-version
//!
//! # Blocking — esp-idf-hal, FreeRTOS tasks, bare-metal loops
//! modbus-bridge = { version = "0.3.2", default-features = false, features = ["sync", "log"] } # x-release-please-version
//! ```
//!
//! `async` and `sync` are mutually exclusive — enable exactly one.
//!
//! # Quick start — Embassy + embassy-net
//!
//! This example shows a complete Modbus TCP gateway task for any Embassy target
//! (ESP32, STM32, RP2040, …). The UART and TCP socket are represented by the
//! `embedded_io_async` traits, so the code is portable across HALs.
//!
//! ```rust,ignore
//! use modbus_bridge::{Bridge, BridgeError, BridgeEvent};
//!
//! #[embassy_executor::task]
//! async fn modbus_gateway(
//!     stack: embassy_net::Stack<'static>,
//!     // Any UART implementing embedded_io_async, e.g. from esp-hal or embassy-stm32.
//!     uart: impl embedded_io_async::Read + embedded_io_async::Write + 'static,
//!     // RS-485 direction-control pin. Pass `modbus_bridge::NoPin` if not needed.
//!     tx_en: impl embedded_hal::digital::OutputPin + 'static,
//! ) {
//!     let mut bridge = Bridge::builder()
//!         .rtu(uart, tx_en)
//!         .build();
//!
//!     // Allocate the TCP socket using the exported buffer-size constants.
//!     let mut rx_buf = [0u8; modbus_bridge::TCP_SOCKET_RX_BUF];
//!     let mut tx_buf = [0u8; modbus_bridge::TCP_SOCKET_TX_BUF];
//!     let mut socket = embassy_net::tcp::TcpSocket::new(stack, &mut rx_buf, &mut tx_buf);
//!
//!     loop {
//!         // Wait for a Modbus TCP client to connect on the standard port 502.
//!         if socket.accept(502).await.is_err() {
//!             socket.abort();
//!             continue;
//!         }
//!
//!         // `accept` borrows `bridge` for the lifetime of the connection and
//!         // takes ownership of the socket.
//!         let mut conn = bridge.accept(socket);
//!
//!         loop {
//!             match conn.next().await {
//!                 // A complete request/response cycle finished successfully.
//!                 Ok(BridgeEvent::Transaction(t)) => defmt::info!("modbus: {}", t),
//!                 // Non-fatal anomaly (e.g. transaction ID mismatch) — still running.
//!                 Ok(BridgeEvent::Warning(w))     => defmt::warn!("modbus: {}", w),
//!                 // TCP client disconnected cleanly — break and accept next client.
//!                 Err(BridgeError::TcpClosed)     => break,
//!                 // Hard error — log it and terminate the connection.
//!                 Err(e) => {
//!                     defmt::error!("modbus error: {}", e);
//!                     break;
//!                 }
//!             }
//!         }
//!
//!         // Recover the socket so it can accept the next client.
//!         socket = conn.into_stream();
//!         socket.close();
//!     }
//! }
//! ```
//!
//! ## Hardware without an RS-485 TX-enable pin
//!
//! Many USB-to-RS-485 adapters and UART peripherals with automatic direction
//! control do not need an explicit TX-enable signal. Use
//! [`BridgeBuilder::rtu_no_pin`] as a shorthand, or pass [`NoPin`] explicitly:
//!
//! ```rust,ignore
//! // Shorthand
//! let mut bridge = Bridge::builder().rtu_no_pin(uart).build();
//!
//! // Equivalent explicit form
//! let mut bridge = Bridge::builder().rtu(uart, modbus_bridge::NoPin).build();
//! ```
//!
//! # Blocking (sync) usage
//!
//! Compile with `default-features = false, features = ["sync"]`. The API is
//! identical: every `.next().await` becomes `.next()` and there is no executor
//! or async runtime required.
//!
//! ```rust,ignore
//! use modbus_bridge::{Bridge, BridgeError, BridgeEvent};
//!
//! let mut bridge = Bridge::builder().rtu(uart, tx_en).build();
//!
//! loop {
//!     // Accept a connection from your blocking TCP stack.
//!     let stream = tcp_listener.accept().unwrap();
//!     let mut conn = bridge.accept(stream);
//!
//!     loop {
//!         match conn.next() {
//!             Ok(BridgeEvent::Transaction(t)) => log::info!("modbus: {t}"),
//!             Ok(BridgeEvent::Warning(w))     => log::warn!("modbus: {w}"),
//!             Err(BridgeError::TcpClosed)     => break,
//!             Err(e) => { log::error!("modbus error: {e}"); break; }
//!         }
//!     }
//! }
//! ```
//!
//! # Feature flags
//!
//! | Feature | Default | Description |
//! |---------|---------|-------------|
//! | `async` | yes | Async transport via [`embedded_io_async`]. Mutually exclusive with `sync`. |
//! | `sync`  | no  | Blocking transport via [`embedded_io`]. Mutually exclusive with `async`. |
//! | `defmt` | no  | Structured logging via [`defmt`] over RTT. Recommended for bare-metal targets. |
//! | `log`   | no  | Logging via the [`log`] facade. Suitable for Linux, esp-idf, and RTOS targets. |
//!
//! # Logging
//!
//! Enable `defmt` (bare-metal / probe-rs RTT) or `log` (standard logger) to
//! receive `info`-level messages for each RTU and TCP frame, and `error`-level
//! messages on I/O failures. Without either feature the crate produces no
//! output at all.
//!
//! # TCP socket buffer sizing
//!
//! When allocating a TCP socket for `embassy-net` or `smoltcp`, pass
//! [`TCP_SOCKET_RX_BUF`] and [`TCP_SOCKET_TX_BUF`] (512 bytes each) as the
//! socket's internal buffer sizes. They are sized to hold one maximum-length
//! Modbus TCP frame (261 bytes) with headroom for TCP ACK latency and a
//! pipelined follow-on request.
//!
//! For computing Modbus *frame* sizes at compile time, see the [`capacity`]
//! module.

#![no_std]

// ── Feature guards ────────────────────────────────────────────────────────────

#[cfg(all(feature = "sync", feature = "async"))]
compile_error!("Features `sync` and `async` are mutually exclusive — enable exactly one.");

#[cfg(not(any(feature = "sync", feature = "async")))]
compile_error!("Exactly one of `sync` or `async` must be enabled.");

// ── Private modules ───────────────────────────────────────────────────────────

mod error;
mod frame;
mod rtu;
mod tcp;

// ── Public modules ────────────────────────────────────────────────────────────

pub mod bridge;
pub mod builder;
pub mod capacity;
pub mod client;
pub mod client_builder;
pub mod client_session;
pub mod connection;
pub mod event;

// ── Top-level re-exports ──────────────────────────────────────────────────────

pub use bridge::Bridge;
pub use builder::BridgeBuilder;

/// Returns a [`BridgeBuilder`] for constructing a [`Bridge`].
///
/// Equivalent to [`Bridge::builder()`], but avoids type-inference failures that
/// occur when the compiler cannot deduce the `Bridge` type parameters from
/// context (e.g. in Embassy tasks where the UART type is known only later in
/// the builder chain).
///
/// Prefer this over `Bridge::builder()` in generic or no-infer contexts.
pub fn builder() -> BridgeBuilder<(), (), NoDelay> {
    BridgeBuilder::new()
}
pub use client::Client;
pub use client_builder::ClientBuilder;
pub use client_session::ClientSession;
pub use connection::Connection;
pub use event::{BridgeError, BridgeEvent, FunctionCode, Transaction, Warning};

// ── No-op TX-enable pin ───────────────────────────────────────────────────────

/// No-op TX-enable pin for hardware that does not need RS-485 direction control.
///
/// Pass this to [`BridgeBuilder::rtu`] when your RS-485 transceiver handles bus
/// direction automatically (e.g. auto-direction-control adapters, full-duplex
/// wiring, or RS-232 connections). [`BridgeBuilder::rtu_no_pin`] is a
/// convenience shorthand that inserts `NoPin` for you.
///
/// # Examples
///
/// ```rust,ignore
/// use modbus_bridge::{Bridge, NoPin};
///
/// let mut bridge = Bridge::builder()
///     .rtu(uart, NoPin)
///     .build();
/// ```
pub struct NoPin;

impl embedded_hal::digital::ErrorType for NoPin {
    type Error = core::convert::Infallible;
}

impl embedded_hal::digital::OutputPin for NoPin {
    fn set_high(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
    fn set_low(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}

// ── No-op delay provider ──────────────────────────────────────────────────────

/// No-op delay provider — the default when no timeout is configured.
///
/// Pass `NoDelay` (or omit the third generic) when you do not need RTU or TCP
/// timeouts. `NoDelay` does **not** implement any delay trait; this is
/// intentional — it enables disjoint `impl` blocks in `Connection` and
/// `ClientSession` without requiring language specialization.
///
/// To enable timeouts, call `.delay(my_delay)` on the builder and set
/// `.rtu_timeout(ms)` and/or `.tcp_timeout(ms)`.
pub struct NoDelay;

// ── TCP socket buffer sizing constants ───────────────────────────────────────

/// Recommended receive-buffer size for the underlying TCP socket (512 bytes).
///
/// Sized to hold one maximum-length Modbus TCP frame (261 bytes: 255-byte RTU
/// PDU + 6-byte MBAP header), rounded to the next power of two with headroom
/// for TCP ACK latency and a pipelined follow-on request.
///
/// Pass this constant when constructing your `TcpSocket` in `embassy-net` or
/// `smoltcp`:
///
/// ```rust,ignore
/// use modbus_bridge::{TCP_SOCKET_RX_BUF, TCP_SOCKET_TX_BUF};
///
/// let mut rx_buf = [0u8; TCP_SOCKET_RX_BUF];
/// let mut tx_buf = [0u8; TCP_SOCKET_TX_BUF];
/// let socket = embassy_net::tcp::TcpSocket::new(stack, &mut rx_buf, &mut tx_buf);
/// ```
pub const TCP_SOCKET_RX_BUF: usize = 512;
/// Recommended transmit-buffer size for the underlying TCP socket (512 bytes).
///
/// See [`TCP_SOCKET_RX_BUF`] for sizing rationale and usage.
pub const TCP_SOCKET_TX_BUF: usize = 512;

// ── Internal logging ──────────────────────────────────────────────────────────

#[cfg(feature = "defmt")]
macro_rules! mb_info {
    ($($t:tt)*) => { defmt::info!($($t)*) };
}
#[cfg(feature = "defmt")]
macro_rules! mb_error {
    ($($t:tt)*) => { defmt::error!($($t)*) };
}

#[cfg(all(not(feature = "defmt"), feature = "log"))]
macro_rules! mb_info {
    ($($t:tt)*) => { log::info!($($t)*) };
}
#[cfg(all(not(feature = "defmt"), feature = "log"))]
macro_rules! mb_error {
    ($($t:tt)*) => { log::error!($($t)*) };
}

#[cfg(not(any(feature = "defmt", feature = "log")))]
#[expect(unused_macros, reason = "only called under defmt or log feature gates")]
macro_rules! mb_info {
    ($($t:tt)*) => {{ let _ = format_args!($($t)*); }};
}
#[cfg(not(any(feature = "defmt", feature = "log")))]
macro_rules! mb_error {
    ($($t:tt)*) => {{ let _ = format_args!($($t)*); }};
}

pub(crate) use mb_error;
#[cfg(any(feature = "defmt", feature = "log"))]
pub(crate) use mb_info;

// ── Fuzzing surface (hidden from public docs) ─────────────────────────────────

/// Internal module exposing frame primitives for fuzz targets.
///
/// Not part of the public API — stability not guaranteed.
#[doc(hidden)]
pub mod __fuzzing {
    pub use crate::frame::{
        check_crc, crc, rtu_resp_to_tcp, rtu_response_remaining, rtu_to_tcp, tcp_resp_to_rtu,
        tcp_to_rtu,
    };
}
