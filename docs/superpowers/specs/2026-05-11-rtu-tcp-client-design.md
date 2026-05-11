# RTU→TCP Client — Design Spec

**Date:** 2026-05-11  
**Status:** Approved

---

## Summary

Add a `Client<S, TX>` type that mirrors `Bridge<S, TX>` in the opposite
direction. Where `Bridge` accepts Modbus TCP connections and proxies requests to
a serial RTU device, `Client` listens on the serial bus for RTU requests from a
Modbus master and proxies them to an upstream Modbus TCP server.

---

## Architecture

The implementation is a strict structural mirror of the existing `Bridge` /
`Connection` / `BridgeBuilder` trio.

| Bridge (TCP→RTU)         | Client (RTU→TCP)              |
|--------------------------|-------------------------------|
| `Bridge<S, TX>`          | `Client<S, TX>`               |
| `BridgeBuilder<S, TX>`   | `ClientBuilder<S, TX>`        |
| `Connection<'b,S,TX,TS>` | `ClientSession<'b,S,TX,TS>`   |
| `bridge.accept(stream)`  | `client.connect(stream)`      |
| `conn.next()`            | `session.next()`              |
| `conn.into_stream()`     | `session.into_stream()`       |

`Client<S, TX>` owns the RTU serial port and RS-485 TX-enable pin.  
The TCP stream (connection to the upstream Modbus TCP server) is passed by the
caller to `client.connect(stream)`, which returns a `ClientSession`. The caller
owns the TCP lifecycle — when to connect, when to reconnect.

Only one `ClientSession` can be active at a time. The `ClientSession` mutably
borrows `Client` for its lifetime, identical to how `Connection` borrows
`Bridge`.

---

## Per-cycle Flow (`ClientSession::next`)

One call to `next()` drives one complete request/response cycle:

1. Listen on the serial bus for a complete RTU request from a Modbus master
   (CRC verified).
2. Assign a wrapping transaction ID and encode as a Modbus TCP frame (MBAP +
   PDU, no CRC).
3. Send the TCP frame to the upstream server.
4. Read the TCP response (MBAP length-prefixed).
5. Verify the transaction ID matches; on mismatch emit
   `BridgeEvent::Warning(Warning::TransactionIdMismatch)` and continue.
6. Strip MBAP, re-add CRC-16, send the RTU response back to the serial master.
7. Return `BridgeEvent::Transaction` describing the proxied request.

---

## New Files

| File | Contents |
|------|----------|
| `src/client.rs` | `Client<S, TX>` struct; `builder()`, `connect()`, `into_inner()` — async and sync cfg blocks |
| `src/client_builder.rs` | `ClientBuilder<S, TX>` typestate builder — identical pattern to `builder.rs` |
| `src/client_session.rs` | `ClientSession<'b,S,TX,TS>`; `next()` (async+sync), `into_stream()` |

---

## Public API additions (`src/lib.rs`)

```rust
pub mod client;
pub mod client_builder;
pub mod client_session;

pub use client::Client;
pub use client_builder::ClientBuilder;
pub use client_session::ClientSession;
```

All existing event/error types (`BridgeEvent`, `BridgeError`, `Transaction`,
`Warning`, `FunctionCode`) are reused unchanged.

---

## Cleanup: remove `flow.rs`

The dead `bridge_flow` and `client_flow` functions in `src/flow.rs` are removed.
`Connection::next()` already inlines the bridge logic; `ClientSession::next()`
will do the same. Removing `flow.rs` eliminates the remaining dead-code warnings
and the unused `mod flow` declaration.

---

## Tests

New `client_tests` module in `tests/integration.rs`, async-gated with
`#[cfg(feature = "async")]`, using the existing `MockStream` and `MockPin`
infrastructure.

**Fixtures** (new, in `fixtures` module):
- `rtu_read_request()` — valid RTU read-holding-registers request with CRC
- `tcp_read_response()` — valid Modbus TCP response (MBAP + PDU)
- `tcp_bad_crc_response()` — `tcp_read_response` with last CRC byte flipped
- `tcp_tid_mismatch_response()` — `tcp_read_response` with transaction ID set to 0xFFFF

**Test cases:**
1. `next_returns_transaction_on_happy_path` — full RTU→TCP→RTU cycle succeeds
2. `next_returns_rtu_closed_on_empty_rtu_stream` — empty RTU stream → `BridgeError::TcpClosed`
3. `next_returns_rtu_crc_mismatch_on_bad_tcp_response` — bad CRC in TCP response → `BridgeError::RtuCrcMismatch`
4. `next_returns_warning_on_tid_mismatch` — TID mismatch in TCP response → `BridgeEvent::Warning`
5. `tcp_request_echoes_rtu_unit_id` — verifies unit ID is preserved through the TCP frame
6. `into_stream_returns_tcp_stream_after_next` — TCP stream has bytes written; RTU rx consumed
7. `client_serves_multiple_sequential_sessions` — client re-usable after session ends

---

## Fuzzing

New fuzz target `fuzz/fuzz_targets/fuzz_client_session.rs` — feeds arbitrary
bytes as the inbound **TCP response** (the untrusted input in client mode) and
verifies no panics. Mirrors the existing `fuzz_frame` target.

Add to `fuzz/Cargo.toml`:

```toml
[[bin]]
name = "fuzz_client_session"
path = "fuzz_targets/fuzz_client_session.rs"
test = false
doc = false
```

---

## Error Mapping

A new `BridgeError::RtuClosed` variant is added to distinguish an RTU master
disconnect from a TCP server disconnect:

| Condition | `BridgeError` variant |
|-----------|----------------------|
| RTU stream EOF (master disconnected) | `RtuClosed` *(new variant)* |
| RTU serial I/O error | `RtuIo(SE)` |
| RTU request CRC invalid | `RtuCrcMismatch` |
| TCP server closed the connection | `TcpClosed` |
| TCP server I/O error | `TcpIo(TE)` |
| TCP response frame too large | `BufferOverflow` |
| TID mismatch | `BridgeEvent::Warning` (not an error) |

`RtuClosed` is the normal exit condition in client mode (equivalent to
`TcpClosed` in bridge mode). The `Display` impl for `RtuClosed` reads
`"RTU connection closed"`.

`BridgeError::TcpClosed` in bridge mode continues to mean TCP client EOF,
unchanged.

---

## Out of Scope

- Automatic TCP reconnection (caller responsibility)
- Multi-session / connection pooling
- RTU broadcast (unit ID 0) handling beyond transparent passthrough
