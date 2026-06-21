# modbus-bridge

[![crates.io](https://img.shields.io/crates/v/modbus-bridge.svg)](https://crates.io/crates/modbus-bridge)
[![docs.rs](https://docs.rs/modbus-bridge/badge.svg)](https://docs.rs/modbus-bridge)
[![license](https://img.shields.io/crates/l/modbus-bridge.svg)](LICENSE-MIT)

Portable `no_std` Modbus RTU/TCP bridge — async and blocking.

Bridges Modbus TCP clients to Modbus RTU serial devices (and vice-versa) with no
heap allocation. All internal buffers use fixed-capacity [`heapless`] collections.
Targets Embassy, esp-idf, FreeRTOS, and bare-metal environments equally.

[`heapless`]: https://crates.io/crates/heapless

## Modes

| Mode | Direction | Use when |
|------|-----------|----------|
| **Bridge** | TCP → RTU | A TCP client talks to this device; it forwards to an RTU slave |
| **Client** | RTU → TCP | An RTU master talks to this device; it forwards to a TCP server |

## Quick Start

Add to `Cargo.toml`:

```toml
# Async (Embassy, smoltcp — enabled by default)
modbus-bridge = { version = "0.3.2" } # x-release-please-version

# Blocking (esp-idf-hal, FreeRTOS tasks, bare-metal loops)
modbus-bridge = { version = "0.3.2", default-features = false, features = ["sync"] } # x-release-please-version
```

`async` and `sync` are mutually exclusive — enable exactly one.

### Bridge mode (async — Embassy)

```rust,ignore
use modbus_bridge::{Bridge, BridgeError, BridgeEvent};

#[embassy_executor::task]
async fn modbus_gateway(
    stack: embassy_net::Stack<'static>,
    uart: impl embedded_io_async::Read + embedded_io_async::Write + 'static,
    tx_en: impl embedded_hal::digital::OutputPin + 'static,
) {
    let mut bridge = Bridge::builder()
        .rtu(uart, tx_en)
        .build();

    let mut rx_buf = [0u8; modbus_bridge::TCP_SOCKET_RX_BUF];
    let mut tx_buf = [0u8; modbus_bridge::TCP_SOCKET_TX_BUF];
    let mut socket = embassy_net::tcp::TcpSocket::new(stack, &mut rx_buf, &mut tx_buf);

    loop {
        if socket.accept(502).await.is_err() {
            socket.abort();
            continue;
        }

        let mut conn = bridge.accept(socket);

        loop {
            match conn.next().await {
                Ok(BridgeEvent::Transaction(t)) => defmt::info!("modbus: {}", t),
                Ok(BridgeEvent::Warning(w))     => defmt::warn!("modbus: {}", w),
                Err(BridgeError::TcpClosed)     => break,
                Err(e) => { defmt::error!("modbus error: {}", e); break; }
            }
        }

        socket = conn.into_stream();
        socket.close();
    }
}
```

### Client mode (async)

```rust,ignore
use modbus_bridge::{Client, BridgeError, BridgeEvent};

let mut client = Client::builder()
    .rtu(uart, tx_en_pin)
    .build();

// tcp_stream connects to the upstream Modbus TCP server
let mut session = client.connect(tcp_stream);
loop {
    match session.next().await {
        Ok(BridgeEvent::Transaction(t)) => log::info!("modbus: {t}"),
        Ok(BridgeEvent::Warning(w))     => log::warn!("modbus: {w}"),
        Err(BridgeError::RtuClosed)     => break,
        Err(e) => { log::error!("{e}"); break; }
    }
}
let tcp_stream = session.into_stream();
```

### Blocking (sync)

Compile with `default-features = false, features = ["sync"]`. The API is identical —
replace every `.await` with nothing, and omit the async executor.

```rust,ignore
use modbus_bridge::{Bridge, BridgeError, BridgeEvent};

let mut bridge = Bridge::builder().rtu(uart, tx_en).build();

loop {
    let stream = tcp_listener.accept().unwrap();
    let mut conn = bridge.accept(stream);
    loop {
        match conn.next() {
            Ok(BridgeEvent::Transaction(t)) => log::info!("modbus: {t}"),
            Ok(BridgeEvent::Warning(w))     => log::warn!("modbus: {w}"),
            Err(BridgeError::TcpClosed)     => break,
            Err(e) => { log::error!("{e}"); break; }
        }
    }
}
```

## Timeouts

Configure per-operation deadlines with `.rtu_timeout()`, `.tcp_timeout()`, and `.delay()`:

```rust,ignore
let bridge = Bridge::builder()
    .rtu(uart, tx_en)
    .rtu_timeout(500)   // 500 ms for RTU device response
    .tcp_timeout(5000)  // 5 s for incoming TCP request
    .delay(my_timer)    // embedded_hal_async::delay::DelayNs (async) or embedded_hal::delay::DelayNs (sync)
    .build();
```

Without `.delay()`, timeouts are disabled regardless of the `ms` values.

## Hardware

### RS-485 TX-enable pin

If your transceiver handles direction control automatically, use `NoPin`:

```rust,ignore
// Shorthand
let bridge = Bridge::builder().rtu_no_pin(uart).build();

// Equivalent
let bridge = Bridge::builder().rtu(uart, modbus_bridge::NoPin).build();
```

### TCP socket buffer sizing

When allocating a TCP socket for `embassy-net` or `smoltcp`, use the exported constants:

```rust,ignore
use modbus_bridge::{TCP_SOCKET_RX_BUF, TCP_SOCKET_TX_BUF};

let mut rx_buf = [0u8; TCP_SOCKET_RX_BUF]; // 512 B
let mut tx_buf = [0u8; TCP_SOCKET_TX_BUF]; // 512 B
```

## Feature Flags

| Feature | Default | Description |
|---------|---------|-------------|
| `async` | yes | Async I/O via [`embedded_io_async`] |
| `sync`  | no  | Blocking I/O via [`embedded_io`] |
| `defmt` | no  | Structured logging via [`defmt`] |
| `log`   | no  | Logging via the [`log`] facade |

[`embedded_io_async`]: https://crates.io/crates/embedded-io-async
[`embedded_io`]: https://crates.io/crates/embedded-io
[`defmt`]: https://crates.io/crates/defmt
[`log`]: https://crates.io/crates/log

## License

Licensed under either of

- [MIT License](LICENSE-MIT)
- [Apache License, Version 2.0](LICENSE-APACHE)

at your option.
