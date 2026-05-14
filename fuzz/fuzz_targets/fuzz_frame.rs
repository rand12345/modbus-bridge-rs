#![no_main]

use libfuzzer_sys::fuzz_target;
use modbus_bridge::__fuzzing as frame;

// Feed arbitrary bytes into every pure frame function and assert no panics.
// The goal is to catch index-out-of-bounds, arithmetic overflow, or any other
// panic reachable from safe code. All functions are expected to either return
// Ok or a typed ModbusError — never panic.
fuzz_target!(|data: &[u8]| {
    // CRC over any input must not panic.
    let _ = frame::crc(data);

    // Conversion functions: feed the same raw bytes as if they were frames.
    let _ = frame::check_crc(data);
    let _ = frame::rtu_to_tcp(data, 0x0001);
    let _ = frame::rtu_to_tcp(data, u16::MAX);
    let _ = frame::tcp_to_rtu(data);
    let _ = frame::tcp_resp_to_rtu(data, 0x0001);
    let _ = frame::rtu_resp_to_tcp(data, 0x0001);

    // rtu_response_remaining only accepts exactly 3 bytes.
    if let Ok(header) = <[u8; 3]>::try_from(data) {
        let _ = frame::rtu_response_remaining(&header);
    }
});
