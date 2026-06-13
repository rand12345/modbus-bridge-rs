//! Integration tests for `modbus_bridge` — exercises the public Bridge API.
//!
//! Modules:
//!   mock         — shared I/O test doubles (async feature only)
//!   fixtures     — reusable frame builders
//!   bridge_tests — Bridge/Connection happy path, hard errors, multi-cycle (async)
//!   event_tests  — FunctionCode, Transaction, Warning, BridgeEvent, BridgeError

// ── Minimal error type (feature-independent) ──────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MockError;

impl core::fmt::Display for MockError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("mock I/O error")
    }
}

impl std::error::Error for MockError {}

// ── Mock I/O (async only) ─────────────────────────────────────────────────────

#[cfg(feature = "async")]
mod mock {
    use super::MockError;
    use embedded_hal::digital::OutputPin;
    use embedded_io_async::{ErrorKind, ErrorType, Read, Write};
    use std::collections::VecDeque;

    impl embedded_io_async::Error for MockError {
        fn kind(&self) -> ErrorKind {
            ErrorKind::Other
        }
    }

    pub struct MockStream {
        pub rx: VecDeque<u8>,
        pub tx: Vec<u8>,
    }

    impl MockStream {
        pub fn with_rx(data: &[u8]) -> Self {
            Self {
                rx: data.iter().copied().collect(),
                tx: Vec::new(),
            }
        }
    }

    impl ErrorType for MockStream {
        type Error = MockError;
    }

    impl Read for MockStream {
        async fn read(&mut self, buf: &mut [u8]) -> Result<usize, MockError> {
            if self.rx.is_empty() {
                return Ok(0);
            }
            let n = buf.len().min(self.rx.len());
            for slot in buf[..n].iter_mut() {
                *slot = self.rx.pop_front().unwrap();
            }
            Ok(n)
        }
    }

    impl Write for MockStream {
        async fn write(&mut self, buf: &[u8]) -> Result<usize, MockError> {
            self.tx.extend_from_slice(buf);
            Ok(buf.len())
        }
        async fn flush(&mut self) -> Result<(), MockError> {
            Ok(())
        }
    }

    pub struct MockPin;

    impl embedded_hal::digital::ErrorType for MockPin {
        type Error = core::convert::Infallible;
    }

    impl OutputPin for MockPin {
        fn set_high(&mut self) -> Result<(), Self::Error> {
            Ok(())
        }
        fn set_low(&mut self) -> Result<(), Self::Error> {
            Ok(())
        }
    }

    /// A read-only stream whose reads never resolve — simulates a stalled device.
    pub struct StallStream;

    impl ErrorType for StallStream {
        type Error = MockError;
    }

    impl Read for StallStream {
        async fn read(&mut self, _buf: &mut [u8]) -> Result<usize, MockError> {
            core::future::pending().await
        }
    }

    impl Write for StallStream {
        async fn write(&mut self, buf: &[u8]) -> Result<usize, MockError> {
            Ok(buf.len())
        }
        async fn flush(&mut self) -> Result<(), MockError> {
            Ok(())
        }
    }

    /// A delay provider that resolves immediately — causes timeouts to fire on the very first poll.
    pub struct InstantDelay;

    impl embedded_hal_async::delay::DelayNs for InstantDelay {
        async fn delay_ns(&mut self, _ns: u32) {}
    }
}

// ── CRC helper (mirrors private frame::crc, used by fixtures) ────────────────

#[cfg(feature = "async")]
fn crc(data: &[u8]) -> [u8; 2] {
    let mut crc: u16 = 0xFFFF;
    for &byte in data {
        crc ^= byte as u16;
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xA001
            } else {
                crc >> 1
            };
        }
    }
    crc.to_le_bytes()
}

// ── Fixtures (used by bridge_tests which are async-gated) ────────────────────

#[cfg(feature = "async")]
mod fixtures {
    use crate::crc;

    /// Modbus TCP request: ReadHoldingRegisters (FC 0x03), unit=1, addr=0, count=2, tid=1.
    pub fn tcp_read_request() -> Vec<u8> {
        vec![
            0x00, 0x01, // tid = 1
            0x00, 0x00, // Modbus protocol identifier
            0x00, 0x06, // PDU length = 6
            0x01, 0x03, 0x00, 0x00, 0x00, 0x02,
        ]
    }

    /// RTU response: unit=1, FC=03, byte_count=4, data=[0x00,0x01,0x00,0x02] + CRC.
    pub fn rtu_read_response() -> Vec<u8> {
        let pdu = [0x01u8, 0x03, 0x04, 0x00, 0x01, 0x00, 0x02];
        let [clo, chi] = crc(&pdu);
        let mut v = pdu.to_vec();
        v.extend_from_slice(&[clo, chi]);
        v
    }

    /// `rtu_read_response` with the final CRC byte flipped.
    pub fn rtu_bad_crc_response() -> Vec<u8> {
        let mut v = rtu_read_response();
        *v.last_mut().unwrap() ^= 0xFF;
        v
    }
}

#[cfg(feature = "async")]
mod client_fixtures {
    use crate::crc;

    /// RTU request from serial master: unit=1, FC=0x03 ReadHoldingRegisters,
    /// start_addr=0, count=1. Eight bytes including CRC.
    pub fn rtu_read_request() -> Vec<u8> {
        let pdu = [0x01u8, 0x03, 0x00, 0x00, 0x00, 0x01];
        let [lo, hi] = crc(&pdu);
        let mut v = pdu.to_vec();
        v.push(lo);
        v.push(hi);
        v
    }

    /// TCP response from upstream server: TID=1, unit=1, FC=03, 1 register = 0x0005.
    pub fn tcp_read_response() -> Vec<u8> {
        vec![
            0x00, 0x01, // TID = 1
            0x00, 0x00, // Modbus protocol identifier
            0x00, 0x05, // PDU length = 5
            0x01, 0x03, 0x02, 0x00, 0x05, // unit=1, FC=3, byte_count=2, data=5
        ]
    }

    /// TCP response with TID=0x0000 instead of 0x0001 — triggers Warning::TransactionIdMismatch.
    /// TCP response with TID=0xFFFF instead of 0x0001 — triggers Warning::TransactionIdMismatch.
    pub fn tcp_tid_mismatch_response() -> Vec<u8> {
        let mut v = tcp_read_response();
        v[0] = 0xFF;
        v[1] = 0xFF;
        v
    }

    /// TCP response where MBAP PDU-length says 10 bytes but stream ends after 3 PDU bytes.
    /// Causes `TcpClosed` (EOF during read).
    pub fn tcp_truncated_response() -> Vec<u8> {
        vec![
            0x00, 0x01, 0x00, 0x00, 0x00, 0x0A, // MBAP: pdu_len=10
            0x01, 0x03, 0x02, // only 3 bytes of PDU — stream ends here
        ]
    }
}

// ── Bridge tests (async only) ─────────────────────────────────────────────────

#[cfg(feature = "async")]
mod bridge_tests {
    use futures::executor::block_on;
    use modbus_bridge::{
        Bridge, BridgeBuilder, BridgeError, BridgeEvent, FunctionCode, Transaction,
    };

    use crate::{
        fixtures,
        mock::{MockPin, MockStream},
    };

    fn make_bridge(serial_rx: &[u8]) -> Bridge<MockStream, MockPin> {
        BridgeBuilder::new()
            .rtu(MockStream::with_rx(serial_rx), MockPin)
            .build()
    }

    #[test]
    fn next_returns_transaction_on_happy_path() {
        block_on(async {
            let mut bridge = make_bridge(&fixtures::rtu_read_response());
            let mut conn = bridge.accept(MockStream::with_rx(&fixtures::tcp_read_request()));

            let event = conn.next().await.expect("next() should succeed");

            assert!(
                matches!(
                    event,
                    BridgeEvent::Transaction(Transaction {
                        unit_id: 1,
                        function_code: FunctionCode::ReadHoldingRegisters,
                        start_address: 0,
                        register_count: 2,
                    })
                ),
                "unexpected event: {:?}",
                event
            );
        });
    }

    #[test]
    fn next_returns_tcp_closed_on_empty_tcp_stream() {
        block_on(async {
            let mut bridge = make_bridge(&[]);
            let mut conn = bridge.accept(MockStream::with_rx(&[]));

            assert!(matches!(conn.next().await, Err(BridgeError::TcpClosed)));
        });
    }

    #[test]
    fn next_returns_rtu_crc_mismatch_on_bad_rtu_response() {
        block_on(async {
            let mut bridge = make_bridge(&fixtures::rtu_bad_crc_response());
            let mut conn = bridge.accept(MockStream::with_rx(&fixtures::tcp_read_request()));

            assert!(matches!(
                conn.next().await,
                Err(BridgeError::RtuCrcMismatch)
            ));
        });
    }

    #[test]
    fn tcp_response_echoes_transaction_id() {
        block_on(async {
            let mut bridge = make_bridge(&fixtures::rtu_read_response());
            let mut conn = bridge.accept(MockStream::with_rx(&fixtures::tcp_read_request()));

            conn.next().await.expect("next() should succeed");

            let stream = conn.into_stream();
            // TCP MBAP header: tid_hi=0x00, tid_lo=0x01 (echoed from request tid=1)
            assert!(stream.tx.len() >= 2, "no response written to TCP stream");
            assert_eq!(&stream.tx[0..2], &[0x00, 0x01], "TID not echoed back");
        });
    }

    #[test]
    fn into_stream_returns_tcp_stream_after_next() {
        block_on(async {
            let mut bridge = make_bridge(&fixtures::rtu_read_response());
            let mut conn = bridge.accept(MockStream::with_rx(&fixtures::tcp_read_request()));

            conn.next().await.unwrap();
            let stream = conn.into_stream();

            // All request bytes were consumed; response was written
            assert!(stream.rx.is_empty());
            assert!(!stream.tx.is_empty());
        });
    }

    #[test]
    fn bridge_serves_multiple_sequential_connections() {
        block_on(async {
            let rtu_resp = fixtures::rtu_read_response();
            let tcp_req = fixtures::tcp_read_request();

            // Pre-load two consecutive RTU responses
            let mut serial_data = rtu_resp.clone();
            serial_data.extend_from_slice(&rtu_resp);

            let mut bridge = make_bridge(&serial_data);

            for i in 0..2 {
                let mut conn = bridge.accept(MockStream::with_rx(&tcp_req));
                let result = conn.next().await;
                assert!(result.is_ok(), "iteration {i} failed: {:?}", result);
            }
        });
    }

    #[test]
    fn bridge_builds_and_works() {
        block_on(async {
            let mut bridge = BridgeBuilder::new()
                .rtu(MockStream::with_rx(&fixtures::rtu_read_response()), MockPin)
                .build();

            let mut conn = bridge.accept(MockStream::with_rx(&fixtures::tcp_read_request()));
            assert!(conn.next().await.is_ok());
        });
    }
}

#[cfg(feature = "async")]
mod client_tests {
    use futures::executor::block_on;
    use modbus_bridge::{
        BridgeError, BridgeEvent, Client, ClientBuilder, FunctionCode, Transaction,
    };

    use crate::{
        client_fixtures,
        mock::{MockPin, MockStream},
    };

    fn make_client(serial_rx: &[u8]) -> Client<MockStream, MockPin> {
        ClientBuilder::new()
            .rtu(MockStream::with_rx(serial_rx), MockPin)
            .build()
    }

    #[test]
    fn next_returns_transaction_on_happy_path() {
        block_on(async {
            let mut client = make_client(&client_fixtures::rtu_read_request());
            let mut session =
                client.connect(MockStream::with_rx(&client_fixtures::tcp_read_response()));

            let event = session.next().await.expect("next() should succeed");

            assert!(
                matches!(
                    event,
                    BridgeEvent::Transaction(Transaction {
                        unit_id: 1,
                        function_code: FunctionCode::ReadHoldingRegisters,
                        start_address: 0,
                        register_count: 1,
                    })
                ),
                "unexpected event: {:?}",
                event
            );
        });
    }

    #[test]
    fn next_returns_rtu_closed_on_empty_rtu_stream() {
        block_on(async {
            let mut client = make_client(&[]);
            let mut session =
                client.connect(MockStream::with_rx(&client_fixtures::tcp_read_response()));

            assert!(matches!(session.next().await, Err(BridgeError::RtuClosed)));
        });
    }

    #[test]
    fn next_returns_tcp_closed_on_truncated_tcp_response() {
        block_on(async {
            let mut client = make_client(&client_fixtures::rtu_read_request());
            let mut session = client.connect(MockStream::with_rx(
                &client_fixtures::tcp_truncated_response(),
            ));

            assert!(matches!(session.next().await, Err(BridgeError::TcpClosed)));
        });
    }

    #[test]
    fn next_returns_warning_on_tid_mismatch() {
        block_on(async {
            let mut client = make_client(&client_fixtures::rtu_read_request());
            let mut session = client.connect(MockStream::with_rx(
                &client_fixtures::tcp_tid_mismatch_response(),
            ));

            let event = session.next().await.expect("should succeed with warning");
            assert!(
                matches!(event, BridgeEvent::Warning(_)),
                "expected Warning, got {:?}",
                event
            );
        });
    }

    #[test]
    fn tcp_request_sent_contains_rtu_unit_id() {
        block_on(async {
            let mut client = make_client(&client_fixtures::rtu_read_request());
            let tcp_stream = MockStream::with_rx(&client_fixtures::tcp_read_response());
            let mut session = client.connect(tcp_stream);

            session.next().await.expect("ok");

            let stream = session.into_stream();
            // TCP MBAP layout: [tid_hi, tid_lo, proto_hi, proto_lo, len_hi, len_lo, UNIT_ID, FC, ...]
            assert!(stream.tx.len() >= 7, "no TCP request written");
            assert_eq!(stream.tx[6], 1, "unit_id not propagated to TCP frame");
        });
    }

    #[test]
    fn into_stream_returns_tcp_stream_with_bytes_written() {
        block_on(async {
            let mut client = make_client(&client_fixtures::rtu_read_request());
            let mut session =
                client.connect(MockStream::with_rx(&client_fixtures::tcp_read_response()));

            session.next().await.expect("ok");
            let stream = session.into_stream();

            assert!(stream.rx.is_empty(), "TCP stream not fully consumed");
            assert!(!stream.tx.is_empty(), "no bytes written to TCP stream");
        });
    }

    #[test]
    fn client_serves_multiple_sequential_sessions() {
        block_on(async {
            let rtu_req = client_fixtures::rtu_read_request();
            let tcp_resp = client_fixtures::tcp_read_response();

            // Pre-load two consecutive RTU requests on the serial port
            let mut serial_data = rtu_req.clone();
            serial_data.extend_from_slice(&rtu_req);

            let mut client = make_client(&serial_data);

            for i in 0..2 {
                let mut session = client.connect(MockStream::with_rx(&tcp_resp));
                let result = session.next().await;
                assert!(result.is_ok(), "iteration {i} failed: {:?}", result);
            }
        });
    }
}

// ── Event type tests ──────────────────────────────────────────────────────────

mod event_tests {
    use modbus_bridge::{BridgeError, BridgeEvent, FunctionCode, Transaction, Warning};

    use crate::MockError;

    #[test]
    fn function_code_from_known_bytes() {
        assert_eq!(FunctionCode::from(0x01), FunctionCode::ReadCoils);
        assert_eq!(FunctionCode::from(0x02), FunctionCode::ReadDiscreteInputs);
        assert_eq!(FunctionCode::from(0x03), FunctionCode::ReadHoldingRegisters);
        assert_eq!(FunctionCode::from(0x04), FunctionCode::ReadInputRegisters);
        assert_eq!(FunctionCode::from(0x05), FunctionCode::WriteSingleCoil);
        assert_eq!(FunctionCode::from(0x06), FunctionCode::WriteSingleRegister);
        assert_eq!(FunctionCode::from(0x0F), FunctionCode::WriteMultipleCoils);
        assert_eq!(
            FunctionCode::from(0x10),
            FunctionCode::WriteMultipleRegisters
        );
        assert_eq!(FunctionCode::from(0xAB), FunctionCode::Other(0xAB));
    }

    #[test]
    fn function_code_display_named() {
        assert_eq!(
            FunctionCode::ReadHoldingRegisters.to_string(),
            "ReadHoldingRegisters"
        );
        assert_eq!(
            FunctionCode::WriteMultipleRegisters.to_string(),
            "WriteMultipleRegisters"
        );
    }

    #[test]
    fn function_code_display_other() {
        let s = FunctionCode::Other(0xAB).to_string();
        assert!(s.contains("ab"), "expected hex 'ab' in '{s}'");
    }

    #[test]
    fn transaction_display_contains_all_fields() {
        let t = Transaction {
            unit_id: 3,
            function_code: FunctionCode::ReadHoldingRegisters,
            start_address: 100,
            register_count: 10,
        };
        let s = t.to_string();
        assert!(s.contains("3"), "unit_id missing in '{s}'");
        assert!(s.contains("ReadHoldingRegisters"), "fc missing in '{s}'");
        assert!(s.contains("100"), "start_address missing in '{s}'");
        assert!(s.contains("10"), "register_count missing in '{s}'");
    }

    #[test]
    fn warning_tid_mismatch_display() {
        let w = Warning::TransactionIdMismatch {
            expected: 5,
            got: 9,
        };
        let s = w.to_string();
        assert!(s.contains('5'), "expected TID missing in '{s}'");
        assert!(s.contains('9'), "actual TID missing in '{s}'");
    }

    #[test]
    fn bridge_event_transaction_display_delegates() {
        let t = Transaction {
            unit_id: 1,
            function_code: FunctionCode::ReadCoils,
            start_address: 0,
            register_count: 1,
        };
        assert_eq!(BridgeEvent::Transaction(t).to_string(), t.to_string());
    }

    #[test]
    fn bridge_event_warning_display_delegates() {
        let w = Warning::TransactionIdMismatch {
            expected: 1,
            got: 2,
        };
        assert_eq!(BridgeEvent::Warning(w).to_string(), w.to_string());
    }

    #[test]
    fn bridge_error_variants_display_non_empty() {
        let cases: &[BridgeError<MockError, MockError>] = &[
            BridgeError::TcpClosed,
            BridgeError::RtuClosed,
            BridgeError::RtuCrcMismatch,
            BridgeError::BufferOverflow,
            BridgeError::Timeout,
            BridgeError::TcpIo(MockError),
            BridgeError::RtuIo(MockError),
        ];
        for e in cases {
            let s = e.to_string();
            assert!(!s.is_empty(), "empty Display for {:?}", e);
        }
    }

    #[test]
    fn bridge_error_debug() {
        let e: BridgeError<MockError, MockError> = BridgeError::TcpClosed;
        let _ = format!("{:?}", e);
    }

    #[test]
    fn transaction_debug() {
        let t = Transaction {
            unit_id: 1,
            function_code: FunctionCode::ReadCoils,
            start_address: 0,
            register_count: 1,
        };
        let _ = format!("{:?}", t);
    }

    #[test]
    fn bridge_event_debug() {
        let t = Transaction {
            unit_id: 1,
            function_code: FunctionCode::ReadCoils,
            start_address: 0,
            register_count: 1,
        };
        let _ = format!("{:?}", BridgeEvent::Transaction(t));
    }

    #[test]
    fn bridge_error_rtu_closed_display() {
        let e: BridgeError<MockError, MockError> = BridgeError::RtuClosed;
        assert_eq!(e.to_string(), "RTU connection closed");
    }

    #[test]
    fn bridge_error_timeout_display() {
        let e: BridgeError<MockError, MockError> = BridgeError::Timeout;
        assert_eq!(e.to_string(), "I/O timeout");
    }
}

#[cfg(feature = "async")]
mod timeout_tests {
    use futures::executor::block_on;
    use modbus_bridge::{BridgeBuilder, BridgeError, ClientBuilder};

    use crate::{
        client_fixtures, fixtures,
        mock::{InstantDelay, MockPin, MockStream, StallStream},
    };

    #[test]
    fn rtu_timeout_returns_timeout_error_for_client() {
        block_on(async {
            // RTU serial port stalls — InstantDelay fires before any bytes arrive.
            let mut client = ClientBuilder::new()
                .rtu(StallStream, MockPin)
                .rtu_timeout(500)
                .delay(InstantDelay)
                .build();

            let mut session =
                client.connect(MockStream::with_rx(&client_fixtures::tcp_read_response()));

            assert!(
                matches!(session.next().await, Err(BridgeError::Timeout)),
                "expected Timeout"
            );
        });
    }

    #[test]
    fn tcp_timeout_returns_timeout_error_for_client() {
        block_on(async {
            // RTU request arrives fine; TCP server response stalls.
            let mut client = ClientBuilder::new()
                .rtu(
                    MockStream::with_rx(&client_fixtures::rtu_read_request()),
                    MockPin,
                )
                .tcp_timeout(500)
                .delay(InstantDelay)
                .build();

            let mut session = client.connect(StallStream);

            assert!(
                matches!(session.next().await, Err(BridgeError::Timeout)),
                "expected Timeout"
            );
        });
    }

    #[test]
    fn no_timeout_with_nodelay_succeeds() {
        block_on(async {
            // No timeout configured, NoDelay used — happy path still works.
            let mut client = ClientBuilder::new()
                .rtu(
                    MockStream::with_rx(&client_fixtures::rtu_read_request()),
                    MockPin,
                )
                .build();

            let mut session =
                client.connect(MockStream::with_rx(&client_fixtures::tcp_read_response()));

            assert!(session.next().await.is_ok(), "expected Ok");
        });
    }

    #[test]
    fn bridge_rtu_timeout_on_slow_device() {
        block_on(async {
            // TCP request arrives fine; RTU device response stalls.
            let mut bridge = BridgeBuilder::new()
                .rtu(StallStream, MockPin)
                .rtu_timeout(500)
                .delay(InstantDelay)
                .build();

            let mut conn = bridge.accept(MockStream::with_rx(&fixtures::tcp_read_request()));

            assert!(
                matches!(conn.next().await, Err(BridgeError::Timeout)),
                "expected Timeout"
            );
        });
    }

    #[test]
    fn bridge_tcp_timeout_on_slow_client() {
        block_on(async {
            // TCP client stalls — no request arrives.
            let mut bridge = BridgeBuilder::new()
                .rtu(MockStream::with_rx(&[]), MockPin)
                .tcp_timeout(500)
                .delay(InstantDelay)
                .build();

            let mut conn = bridge.accept(StallStream);

            assert!(
                matches!(conn.next().await, Err(BridgeError::Timeout)),
                "expected Timeout"
            );
        });
    }
}
