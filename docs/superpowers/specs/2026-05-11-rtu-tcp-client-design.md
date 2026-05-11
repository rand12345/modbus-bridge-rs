# RTU→TCP Client + Optional Timeouts — Design Spec

**Date:** 2026-05-11  
**Status:** Approved

---

## Summary

Two additions:

1. **`Client<S, TX, D>`** — mirrors `Bridge` in the opposite direction. Listens
   on the serial bus for RTU requests from a Modbus master and proxies them to
   an upstream Modbus TCP server.

2. **Optional I/O timeouts** — both `Bridge` and `Client` gain optional
   per-operation RTU and TCP timeouts. Timeouts bound individual I/O waits
   inside `next()`; reconnection policy remains the caller's responsibility.

---

## Architecture

### Client (mirrors Bridge)

| Bridge (TCP→RTU)           | Client (RTU→TCP)                |
|----------------------------|---------------------------------|
| `Bridge<S, TX, D>`         | `Client<S, TX, D>`              |
| `BridgeBuilder<S, TX, D>`  | `ClientBuilder<S, TX, D>`       |
| `Connection<'b,S,TX,TS>`   | `ClientSession<'b,S,TX,TS>`     |
| `bridge.accept(stream)`    | `client.connect(stream)`        |
| `conn.next()`              | `session.next()`                |
| `conn.into_stream()`       | `session.into_stream()`         |

### Timeout generic parameter `D`

Both `Bridge` and `Client` (and their builders) gain a third generic parameter
`D` with a default of `NoDelay`:

```rust
pub struct Bridge<S, TX, D = NoDelay> { ... }
pub struct Client<S, TX, D = NoDelay> { ... }
```

`D` must implement `embedded_hal_async::delay::DelayNs` (async feature) or be
`NoDelay` (sync feature, no-op). Because `D = NoDelay` is the default, all
existing code that writes `Bridge<Uart, Pin>` continues to compile unchanged —
no breaking change.

`NoDelay` is a new exported zero-sized type. Its `DelayNs` impl panics if a
non-zero timeout is set (debug guard), and is a no-op if no timeout is
configured. In practice, `NoDelay` users simply don't call `.rtu_timeout()` or
`.tcp_timeout()`.

---

## Timeout Design

### Builder configuration

Both `BridgeBuilder` and `ClientBuilder` gain three new methods:

```rust
/// Sets the RTU I/O timeout (applied to reading each RTU frame).
pub fn rtu_timeout(self, ms: u32) -> Self;

/// Sets the TCP I/O timeout (applied to reading each TCP response).
pub fn tcp_timeout(self, ms: u32) -> Self;

/// Supplies the async delay provider and upgrades D from NoDelay.
pub fn delay<D2: DelayNs>(self, delay: D2) -> Builder<S, TX, D2>;
```

Timeouts are stored as `Option<u32>` (milliseconds) in the builder and
forwarded to the built `Bridge`/`Client`.

### Application inside `next()`

Timeouts are applied at two points in the cycle using
`futures::future::select` to race the I/O future against a delay future:

| Step | Timeout applied |
|------|----------------|
| RTU frame receive | `rtu_timeout_ms` |
| TCP response receive | `tcp_timeout_ms` |

If the delay fires before I/O completes, `next()` returns
`Err(BridgeError::Timeout)`.

### Sync feature

Sync timeouts are fully supported using two traits already present in the
existing dependencies — no new crates needed:

| Trait | Crate | Purpose |
|-------|-------|---------|
| `embedded_io::ReadReady` | `embedded-io` 0.6 (existing) | Non-blocking readiness check |
| `embedded_hal::delay::DelayNs` | `embedded-hal` 1.0 (existing) | Blocking inter-poll sleep |

Implementation: poll `read_ready()` in a 1ms loop, decrement a countdown,
return `BridgeError::Timeout` when the budget reaches zero.

```
loop {
    if stream.read_ready()? { return read(); }
    if elapsed_ms >= timeout_ms { return Err(Timeout); }
    delay.delay_ms(1);
    elapsed_ms += 1;
}
```

Granularity is ~1ms — appropriate for Modbus (typical response timeouts
200–2000ms).

**Additional bound:** When timeouts are configured in sync mode, `S` must
implement `ReadReady` in addition to `Read + Write`. This bound is only
required on the `impl` block that uses a timeout (`D != NoDelay`), so users
who do not configure timeouts see no API change.

### New dependency

```toml
embedded-hal-async = { version = "1.0", optional = true }
futures-util       = { version = "0.3", default-features = false,
                       features = ["async-await"], optional = true }
```

Both are folded into the existing `async` feature so no new feature flag is
needed.

---

## Per-cycle Flow (`ClientSession::next`)

1. Listen on the serial bus for a complete RTU request (CRC verified).
   → Apply `rtu_timeout_ms` around this read.
2. Assign a transaction ID; encode as Modbus TCP (MBAP + PDU, no CRC).
3. Send TCP frame to the upstream server.
4. Read the TCP response (MBAP length-prefixed).
   → Apply `tcp_timeout_ms` around this read.
5. Verify transaction ID; on mismatch emit `BridgeEvent::Warning`.
6. Strip MBAP, re-add CRC-16, send RTU response to the serial master.
7. Return `BridgeEvent::Transaction`.

---

## New Files

| File | Contents |
|------|----------|
| `src/client.rs` | `Client<S, TX, D>` struct; `builder()`, `connect()`, `into_inner()` |
| `src/client_builder.rs` | `ClientBuilder<S, TX, D>` typestate builder |
| `src/client_session.rs` | `ClientSession<'b,S,TX,TS>`; `next()` (async+sync), `into_stream()` |

### Modified files

| File | Change |
|------|--------|
| `src/lib.rs` | Add new modules + re-exports; add `NoDelay` export |
| `src/bridge.rs` | Add `D` type param; forward timeout/delay from builder |
| `src/builder.rs` | Add `D` type param; add `delay()`, `rtu_timeout()`, `tcp_timeout()` |
| `src/connection.rs` | Apply timeout in `next()` via `D` |
| `src/event.rs` | Add `BridgeError::RtuClosed` and `BridgeError::Timeout` variants |
| `src/flow.rs` | **Delete** (dead code, see below) |
| `Cargo.toml` | Add `embedded-hal-async`, `futures-util` to `async` feature |

---

## Public API additions (`src/lib.rs`)

```rust
pub mod client;
pub mod client_builder;
pub mod client_session;

pub use client::Client;
pub use client_builder::ClientBuilder;
pub use client_session::ClientSession;
pub use NoDelay;          // already exported; ensure visible
```

---

## Cleanup: delete `flow.rs`

`src/flow.rs` contains dead `bridge_flow` and `client_flow` functions whose
logic is inlined directly in `Connection::next()` and will be inlined in
`ClientSession::next()`. Deleting it removes all remaining dead-code warnings
from this module and removes the `mod flow` declaration from `lib.rs`.

---

## Error additions (`src/event.rs`)

```rust
pub enum BridgeError<SE, TE> {
    TcpClosed,           // existing — TCP client/server EOF
    RtuClosed,           // NEW — RTU master EOF (normal exit for Client)
    TcpIo(TE),
    RtuIo(SE),
    RtuCrcMismatch,
    BufferOverflow,
    Timeout,             // NEW — rtu_timeout or tcp_timeout expired
}
```

| Display string | Variant |
|----------------|---------|
| `"RTU connection closed"` | `RtuClosed` |
| `"I/O timeout"` | `Timeout` |

`RtuClosed` is the normal exit condition in client mode (equivalent to
`TcpClosed` in bridge mode).

---

## Error Mapping

| Condition | `BridgeError` variant |
|-----------|----------------------|
| RTU stream EOF | `RtuClosed` |
| RTU serial I/O error | `RtuIo(SE)` |
| RTU request CRC invalid | `RtuCrcMismatch` |
| TCP server EOF | `TcpClosed` |
| TCP server I/O error | `TcpIo(TE)` |
| RTU or TCP timeout expired | `Timeout` |
| Frame too large | `BufferOverflow` |
| TID mismatch | `BridgeEvent::Warning` (non-fatal) |

---

## Tests

### Client tests

New `client_tests` module in `tests/integration.rs` (`#[cfg(feature = "async")]`),
using existing `MockStream` and `MockPin`.

**New fixtures:**
- `rtu_read_request()` — valid RTU FC03 request bytes with CRC
- `tcp_read_response()` — valid Modbus TCP response (MBAP + PDU, tid=1)
- `tcp_bad_crc_response()` — `tcp_read_response` with final byte flipped
- `tcp_tid_mismatch_response()` — `tcp_read_response` with TID set to 0xFFFF

**Test cases:**
1. `next_returns_transaction_on_happy_path`
2. `next_returns_rtu_closed_on_empty_rtu_stream`
3. `next_returns_crc_mismatch_on_bad_tcp_response`
4. `next_returns_warning_on_tid_mismatch`
5. `tcp_request_sent_contains_rtu_unit_id`
6. `into_stream_returns_tcp_stream_with_bytes_written`
7. `client_serves_multiple_sequential_sessions`

### Timeout tests

New `timeout_tests` module in `tests/integration.rs` (`#[cfg(feature = "async")]`).
Uses a `MockDelay` that fires immediately (zero-duration) to force timeout
without real time passing.

**Test cases:**
1. `rtu_timeout_returns_timeout_error` — rtu_timeout set, RTU stream stalls → `Timeout`
2. `tcp_timeout_returns_timeout_error` — tcp_timeout set, TCP stream stalls → `Timeout`
3. `no_timeout_with_nodelay_succeeds` — no timeout configured, `NoDelay` used → no error
4. `bridge_rtu_timeout_on_slow_device` — same as (1) but for `Bridge::accept`
5. `bridge_tcp_timeout_on_slow_client` — same as (2) but for `Bridge::accept`

`MockDelay` implements `DelayNs` and resolves after N polls (configurable), or
immediately when set to 0.

---

## Fuzzing

New fuzz target `fuzz/fuzz_targets/fuzz_client_session.rs` — feeds arbitrary
bytes as the inbound TCP response (untrusted input in client mode). Verifies no
panics. Mirrors existing `fuzz_frame`.

Add to `fuzz/Cargo.toml`:
```toml
[[bin]]
name = "fuzz_client_session"
path = "fuzz_targets/fuzz_client_session.rs"
test = false
doc = false
```

---

## Out of Scope

- Automatic TCP reconnection (caller responsibility)
- Multi-session / connection pooling
- RTU broadcast (unit ID 0) passthrough is transparent, no special handling
