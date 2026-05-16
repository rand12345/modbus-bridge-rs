//! Frame buffer sizing constants and helpers for Modbus RTU and TCP frames.
//!
//! Use the named constants or the `const fn` helpers when you need to know the
//! exact byte size of a Modbus frame for a given register count — for example,
//! when allocating application-level buffers or computing memory budgets.
//!
//! The [`Bridge`](crate::Bridge) itself uses full-spec internal buffers
//! (255 bytes for RTU, 261 bytes for TCP) and does not require any sizing from
//! this module.
//!
//! # Sizing rules
//!
//! For **bridge mode** (TCP → RTU), use the `TCP_*` constants — TCP frames
//! carry a 6-byte MBAP header that RTU frames do not, so TCP is always the
//! larger side.
//!
//! For **RTU-only** use cases, the `RTU_*` constants are sufficient.
//!
//! # Examples
//!
//! Compute a frame size at compile time using the `const fn` helpers:
//!
//! ```rust
//! use modbus_bridge::capacity::{rtu_capacity, tcp_capacity, REG_BYTES_16, REG_BYTES_32};
//!
//! // RTU frame size for 8 × 16-bit holding registers
//! const MY_RTU_BUF: usize = rtu_capacity(8, REG_BYTES_16);
//! assert_eq!(MY_RTU_BUF, 25); // 8×2 + 9 bytes overhead
//!
//! // TCP frame size for the same register count
//! const MY_TCP_BUF: usize = tcp_capacity(8, REG_BYTES_16);
//! assert_eq!(MY_TCP_BUF, 31); // 8×2 + 15 bytes overhead
//!
//! // RTU frame size for 4 × 32-bit register pairs
//! const MY_32BIT_BUF: usize = rtu_capacity(4, REG_BYTES_32);
//! assert_eq!(MY_32BIT_BUF, 25); // 4×4 + 9 bytes overhead
//! ```
//!
//! Or use a named constant directly:
//!
//! ```rust
//! use modbus_bridge::capacity::TCP_8R16;
//!
//! assert_eq!(TCP_8R16, 31);
//! ```

/// Bytes per standard 16-bit Modbus register.
pub const REG_BYTES_16: usize = 2;
/// Bytes per 32-bit value stored as two consecutive Modbus registers.
pub const REG_BYTES_32: usize = 4;

/// Computes the RTU frame buffer size in bytes for `n` registers of `reg_bytes` bytes each.
///
/// Sized for the worst case: a Write Multiple Registers request (FC 0x10):
/// `addr(1) + FC(1) + start(2) + qty(2) + byte_count(1) + data(n×b) + CRC(2) = n×b + 9`
///
/// Pass [`REG_BYTES_16`] for standard 16-bit registers or [`REG_BYTES_32`] for
/// 32-bit values stored across two consecutive registers.
///
/// # Examples
///
/// ```rust
/// use modbus_bridge::capacity::{rtu_capacity, REG_BYTES_16};
///
/// const BUF: usize = rtu_capacity(10, REG_BYTES_16);
/// assert_eq!(BUF, 29); // 10×2 + 9
/// ```
pub const fn rtu_capacity(n: usize, reg_bytes: usize) -> usize {
    n * reg_bytes + 9
}

/// Computes the TCP frame buffer size in bytes for `n` registers of `reg_bytes` bytes each.
///
/// Sized for the worst case: a Write Multiple Registers request (FC 0x10) over TCP:
/// `MBAP(6) + addr(1) + FC(1) + start(2) + qty(2) + byte_count(1) + data(n×b) = n×b + 15`
///
/// The MBAP header adds 6 bytes over the equivalent RTU frame, so TCP buffers
/// are always larger than RTU buffers for the same register count.
///
/// # Examples
///
/// ```rust
/// use modbus_bridge::capacity::{tcp_capacity, REG_BYTES_16};
///
/// const BUF: usize = tcp_capacity(10, REG_BYTES_16);
/// assert_eq!(BUF, 35); // 10×2 + 15
/// ```
pub const fn tcp_capacity(n: usize, reg_bytes: usize) -> usize {
    n * reg_bytes + 15
}

// ── Named RTU constants — 16-bit registers ────────────────────────────────────

/// RTU buffer: 1 × 16-bit register (11 B).
pub const RTU_1R16: usize = rtu_capacity(1, REG_BYTES_16);
/// RTU buffer: 4 × 16-bit registers (17 B).
pub const RTU_4R16: usize = rtu_capacity(4, REG_BYTES_16);
/// RTU buffer: 8 × 16-bit registers (25 B).
pub const RTU_8R16: usize = rtu_capacity(8, REG_BYTES_16);
/// RTU buffer: 10 × 16-bit registers (29 B).
pub const RTU_10R16: usize = rtu_capacity(10, REG_BYTES_16);
/// RTU buffer: 16 × 16-bit registers (41 B).
pub const RTU_16R16: usize = rtu_capacity(16, REG_BYTES_16);
/// RTU buffer: 32 × 16-bit registers (73 B).
pub const RTU_32R16: usize = rtu_capacity(32, REG_BYTES_16);
/// RTU buffer: 64 × 16-bit registers (137 B).
pub const RTU_64R16: usize = rtu_capacity(64, REG_BYTES_16);
/// RTU buffer: Modbus spec maximum — 123 × 16-bit registers (255 B).
pub const RTU_MAX: usize = rtu_capacity(123, REG_BYTES_16);

// ── Named RTU constants — 32-bit register pairs ───────────────────────────────

/// RTU buffer: 1 × 32-bit register pair (13 B).
pub const RTU_1R32: usize = rtu_capacity(1, REG_BYTES_32);
/// RTU buffer: 4 × 32-bit register pairs (25 B).
pub const RTU_4R32: usize = rtu_capacity(4, REG_BYTES_32);
/// RTU buffer: 8 × 32-bit register pairs (41 B).
pub const RTU_8R32: usize = rtu_capacity(8, REG_BYTES_32);
/// RTU buffer: 10 × 32-bit register pairs (49 B).
pub const RTU_10R32: usize = rtu_capacity(10, REG_BYTES_32);
/// RTU buffer: 32 × 32-bit register pairs (137 B).
pub const RTU_32R32: usize = rtu_capacity(32, REG_BYTES_32);
/// RTU buffer: spec maximum for 32-bit pairs — 61 pairs (253 B).
pub const RTU_MAX32: usize = rtu_capacity(61, REG_BYTES_32);

// ── Named TCP constants — 16-bit registers ────────────────────────────────────

/// TCP buffer: 1 × 16-bit register (17 B).
pub const TCP_1R16: usize = tcp_capacity(1, REG_BYTES_16);
/// TCP buffer: 4 × 16-bit registers (23 B).
pub const TCP_4R16: usize = tcp_capacity(4, REG_BYTES_16);
/// TCP buffer: 8 × 16-bit registers (31 B).
pub const TCP_8R16: usize = tcp_capacity(8, REG_BYTES_16);
/// TCP buffer: 10 × 16-bit registers (35 B).
pub const TCP_10R16: usize = tcp_capacity(10, REG_BYTES_16);
/// TCP buffer: 16 × 16-bit registers (47 B).
pub const TCP_16R16: usize = tcp_capacity(16, REG_BYTES_16);
/// TCP buffer: 32 × 16-bit registers (79 B).
pub const TCP_32R16: usize = tcp_capacity(32, REG_BYTES_16);
/// TCP buffer: 64 × 16-bit registers (143 B).
pub const TCP_64R16: usize = tcp_capacity(64, REG_BYTES_16);
/// TCP buffer: Modbus spec maximum — 123 × 16-bit registers (261 B).
pub const TCP_MAX: usize = tcp_capacity(123, REG_BYTES_16);

// ── Named TCP constants — 32-bit register pairs ───────────────────────────────

/// TCP buffer: 10 × 32-bit register pairs (55 B).
pub const TCP_10R32: usize = tcp_capacity(10, REG_BYTES_32);
/// TCP buffer: spec maximum for 32-bit pairs — 61 pairs (259 B).
pub const TCP_MAX32: usize = tcp_capacity(61, REG_BYTES_32);
