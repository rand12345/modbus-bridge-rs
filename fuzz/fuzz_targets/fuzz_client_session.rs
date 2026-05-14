#![no_main]

//! Fuzz the TCP→RTU response conversion path used in `ClientSession::next()`.
//!
//! Feeds arbitrary bytes as the TCP server response. Verifies no panics occur
//! in the frame parsing and conversion functions. Mirrors `fuzz_frame` but
//! focuses on the client-mode code path.

use libfuzzer_sys::fuzz_target;
use modbus_bridge::__fuzzing as frame;

fuzz_target!(|data: &[u8]| {
    // Simulate receiving arbitrary bytes as a TCP response from the server.
    // tcp_resp_to_rtu must never panic regardless of input.
    let _ = frame::tcp_resp_to_rtu(data, 0x0001);
    let _ = frame::tcp_resp_to_rtu(data, 0x0000);
    let _ = frame::tcp_resp_to_rtu(data, u16::MAX);

    // rtu_to_tcp converts the RTU request before sending — fuzz this too.
    let _ = frame::rtu_to_tcp(data, 0x0001);
});
