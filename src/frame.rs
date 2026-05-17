//! Pure Modbus framing: CRC, MBAP encode/decode, RTU↔TCP conversions.
//! No I/O — all functions operate on byte slices.

use crate::error::ModbusError;
use core::convert::Infallible;

/// Variable-length TCP response buffer (internal use).
pub(crate) type TcpResponse = heapless::Vec<u8, 512>;
/// Variable-length RTU response buffer (internal use).
pub(crate) type RtuResponse = heapless::Vec<u8, 512>;

const MODBUS_PROTO: [u8; 2] = [0, 0];

// ── CRC ──────────────────────────────────────────────────────────────────────

/// Compute Modbus CRC-16 and return it as a little-endian `[lo, hi]` pair.
pub fn crc(data: &[u8]) -> [u8; 2] {
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

/// Verify the trailing 2-byte CRC on an RTU frame.
pub fn check_crc(frame: &[u8]) -> Result<(), ModbusError<Infallible, Infallible>> {
    if frame.len() < 4 {
        return Err(ModbusError::PayloadTooShort);
    }
    let (body, rx_crc) = frame.split_at(frame.len() - 2);
    if crc(body) == rx_crc {
        Ok(())
    } else {
        Err(ModbusError::Crc)
    }
}

// ── RTU → TCP ────────────────────────────────────────────────────────────────

/// Wrap an RTU request frame (PDU + CRC) into a Modbus TCP frame (MBAP + PDU, no CRC).
pub fn rtu_to_tcp(
    rtu: &[u8],
    transaction_id: u16,
) -> Result<TcpResponse, ModbusError<Infallible, Infallible>> {
    if rtu.len() < 4 {
        return Err(ModbusError::PayloadTooShort);
    }
    let (pdu, _crc) = rtu.split_at(rtu.len() - 2);
    let len_bytes = (pdu.len() as u16).to_be_bytes();
    let mut out = TcpResponse::new();
    out.extend_from_slice(&transaction_id.to_be_bytes())
        .and(out.extend_from_slice(&MODBUS_PROTO))
        .and(out.extend_from_slice(&len_bytes))
        .and(out.extend_from_slice(pdu))
        .map_err(|_| ModbusError::ConversionSlice)?;
    Ok(out)
}

// ── TCP → RTU ────────────────────────────────────────────────────────────────

/// Unwrap a Modbus TCP request into an RTU frame (PDU + CRC).
/// Returns `(rtu_frame, transaction_id)`.
pub fn tcp_to_rtu(
    tcp: &[u8],
) -> Result<(RtuResponse, u16), ModbusError<Infallible, Infallible>> {
    if tcp.len() < 7 {
        return Err(ModbusError::PayloadTooShort);
    }
    let transaction_id = u16::from_be_bytes([tcp[0], tcp[1]]);
    let pdu = &tcp[6..];
    let mut out = RtuResponse::new();
    out.extend_from_slice(pdu)
        .and(out.extend_from_slice(&crc(pdu)))
        .map_err(|_| ModbusError::ConversionSlice)?;
    Ok((out, transaction_id))
}

// ── TCP response → RTU response ───────────────────────────────────────────────

/// Convert a TCP response into an RTU response (strip MBAP, add CRC).
/// Validates that the response transaction ID matches `expected_tid`.
pub fn tcp_resp_to_rtu(
    tcp: &[u8],
    expected_tid: u16,
) -> Result<RtuResponse, ModbusError<Infallible, Infallible>> {
    if tcp.len() < 7 {
        return Err(ModbusError::PayloadTooShort);
    }
    let rx_tid = u16::from_be_bytes([tcp[0], tcp[1]]);
    if rx_tid != expected_tid {
        return Err(ModbusError::InvalidTransactionId);
    }
    let pdu_len = tcp[5] as usize;
    if tcp.len() < 6 + pdu_len {
        return Err(ModbusError::PayloadTooShort);
    }
    let pdu = &tcp[6..6 + pdu_len];
    let mut out = RtuResponse::new();
    out.extend_from_slice(pdu)
        .and(out.extend_from_slice(&crc(pdu)))
        .map_err(|_| ModbusError::ConversionSlice)?;
    Ok(out)
}

// ── RTU response → TCP response ───────────────────────────────────────────────

/// Convert an RTU response into a TCP response (strip CRC, add MBAP).
pub fn rtu_resp_to_tcp(
    rtu: &[u8],
    transaction_id: u16,
) -> Result<TcpResponse, ModbusError<Infallible, Infallible>> {
    if rtu.len() < 4 {
        return Err(ModbusError::PayloadTooShort);
    }
    let (pdu, _crc) = rtu.split_at(rtu.len() - 2);
    let len_bytes = (pdu.len() as u16).to_be_bytes();
    let mut out = TcpResponse::new();
    out.extend_from_slice(&transaction_id.to_be_bytes())
        .and(out.extend_from_slice(&MODBUS_PROTO))
        .and(out.extend_from_slice(&len_bytes))
        .and(out.extend_from_slice(pdu))
        .map_err(|_| ModbusError::ConversionSlice)?;
    Ok(out)
}

// ── RTU response framing helper ───────────────────────────────────────────────

/// Given the first 3 bytes of an RTU response `[addr, fn_code, byte3]`,
/// return the number of additional bytes to read to complete the frame
/// (payload bytes + 2 CRC bytes).
///
/// RTU response frame layouts:
/// - Read FCs (01–04): `[addr, fc, byte_count, data…, crc(2)]`  → `byte_count + 2` remaining
/// - Write-echo FCs (05, 06, 0F, 10): `[addr, fc, addr_hi, addr_lo, val/qty(2), crc(2)]` → 5 remaining
/// - Exception (fc | 0x80): `[addr, fc|0x80, code, crc(2)]` → 2 remaining
pub fn rtu_response_remaining(header: &[u8; 3]) -> usize {
    match header[1] {
        // Exception responses: FC + 0x80; body is [unit, fc|0x80, code, crc_lo, crc_hi]
        0x80..=0xFF => 2,
        // Write-echo responses: fixed 8-byte frame; after the 3-byte header, 5 bytes remain
        0x05 | 0x06 | 0x0F | 0x10 => 5,
        // Read FCs and any unrecognised FC: header[2] is byte_count
        _ => header[2] as usize + 2,
    }
}

// ── Request parsing helper ────────────────────────────────────────────────────

/// Extract `(unit_id, fc, start_address, register_count)` from a TCP request frame.
///
/// Layout: `[tid_hi, tid_lo, 0, 0, len_hi, len_lo, unit_id, fc, start_hi, start_lo, qty_hi, qty_lo, ...]`
/// For all standard read/write FCs the first six PDU bytes are always addr + fc + start(2) + qty(2).
pub(crate) fn parse_tcp_request(tcp: &[u8]) -> Option<(u8, u8, u16, u16)> {
    if tcp.len() < 12 {
        return None;
    }
    let unit_id = tcp[6];
    let fc = tcp[7];
    let start = u16::from_be_bytes([tcp[8], tcp[9]]);
    let qty = u16::from_be_bytes([tcp[10], tcp[11]]);
    Some((unit_id, fc, start, qty))
}

/// Extract `(unit_id, fc, start_address, register_count)` from an RTU request frame.
///
/// Layout: `[addr, fc, start_hi, start_lo, qty_hi, qty_lo, crc_lo, crc_hi]`
/// For all standard read/write FCs the first six bytes are addr + fc + start(2) + qty(2).
pub(crate) fn parse_rtu_request(rtu: &[u8]) -> Option<(u8, u8, u16, u16)> {
    if rtu.len() < 8 {
        return None;
    }
    let unit_id = rtu[0];
    let fc = rtu[1];
    let start = u16::from_be_bytes([rtu[2], rtu[3]]);
    let qty = u16::from_be_bytes([rtu[4], rtu[5]]);
    Some((unit_id, fc, start, qty))
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // Helper: build a valid RTU frame [addr, fn, d0, d1, d2, d3, crc_lo, crc_hi]
    fn make_rtu_request() -> [u8; 8] {
        let body = [0x01u8, 0x03, 0x00, 0x00, 0x00, 0x02];
        let [lo, hi] = crc(&body);
        [body[0], body[1], body[2], body[3], body[4], body[5], lo, hi]
    }

    // ── crc ──────────────────────────────────────────────────────────────────

    #[test]
    fn crc_self_check_residual() {
        // Appending the computed CRC to data and re-computing yields a fixed residual.
        // For Modbus CRC-16/ARC the residual over [data || crc_lo || crc_hi] is 0x0000.
        let body = [0x01u8, 0x03, 0x00, 0x00, 0x00, 0x02];
        let c = crc(&body);
        let mut full = [0u8; 8];
        full[..6].copy_from_slice(&body);
        full[6..].copy_from_slice(&c);
        assert_eq!(crc(&full), [0x00, 0x00]);
    }

    #[test]
    fn crc_single_zero_byte() {
        // Computed manually: CRC of [0x00] is 0x40BF (le: [0xBF, 0x40]).
        assert_eq!(crc(&[0x00]), [0xBF, 0x40]);
    }

    #[test]
    fn crc_all_ff_single_byte() {
        // CRC of [0xFF]: start=0xFFFF, XOR→0xFF00, 8 shifts→0x00FF (le: [0xFF, 0x00]).
        assert_eq!(crc(&[0xFF]), [0xFF, 0x00]);
    }

    // ── rtu_response_remaining ────────────────────────────────────────────────

    #[test]
    fn remaining_read_fc_zero_byte_count() {
        // FC03 read response with byte_count=0 → only CRC remains.
        assert_eq!(rtu_response_remaining(&[0x01, 0x03, 0x00]), 2);
    }

    #[test]
    fn remaining_read_fc_max_byte_count() {
        assert_eq!(rtu_response_remaining(&[0x01, 0x03, 0xFF]), 257);
    }

    #[test]
    fn remaining_write_single_coil_fc05() {
        // FC05 echo response is 8 bytes total; after 3-byte header, 5 remain.
        assert_eq!(rtu_response_remaining(&[0x01, 0x05, 0x00]), 5);
    }

    #[test]
    fn remaining_write_single_register_fc06() {
        assert_eq!(rtu_response_remaining(&[0x01, 0x06, 0x00]), 5);
    }

    #[test]
    fn remaining_write_multiple_coils_fc0f() {
        assert_eq!(rtu_response_remaining(&[0x01, 0x0F, 0x00]), 5);
    }

    #[test]
    fn remaining_write_multiple_registers_fc10() {
        assert_eq!(rtu_response_remaining(&[0x01, 0x10, 0x00]), 5);
    }

    #[test]
    fn remaining_exception_response() {
        // Exception: FC + 0x80; after 3-byte header only the 2 CRC bytes remain.
        assert_eq!(rtu_response_remaining(&[0x01, 0x83, 0x02]), 2);
        assert_eq!(rtu_response_remaining(&[0x01, 0x85, 0x02]), 2);
        assert_eq!(rtu_response_remaining(&[0x01, 0x90, 0x02]), 2);
    }

    // ── roundtrip invariants ──────────────────────────────────────────────────

    #[test]
    fn rtu_to_tcp_then_tcp_to_rtu_recovers_pdu() {
        let frame = make_rtu_request();
        let tcp = rtu_to_tcp(&frame, 0x0042).unwrap();
        let (rtu2, tid) = tcp_to_rtu(&tcp).unwrap();
        // PDU bytes (strip CRC from originals and from result)
        assert_eq!(&frame[..frame.len() - 2], &rtu2[..rtu2.len() - 2]);
        assert_eq!(tid, 0x0042);
    }

    #[test]
    fn rtu_resp_to_tcp_then_tcp_resp_to_rtu_recovers_pdu() {
        // Build a minimal RTU response: [addr, fn, byte_count, d0, d1, crc_lo, crc_hi]
        let body = [0x01u8, 0x03, 0x02, 0x00, 0x01];
        let [lo, hi] = crc(&body);
        let rtu_resp: &[u8] = &[body[0], body[1], body[2], body[3], body[4], lo, hi];

        let tcp = rtu_resp_to_tcp(rtu_resp, 0x0007).unwrap();
        let rtu2 = tcp_resp_to_rtu(&tcp, 0x0007).unwrap();
        assert_eq!(&rtu_resp[..rtu_resp.len() - 2], &rtu2[..rtu2.len() - 2]);
    }

    #[test]
    fn rtu_to_tcp_transaction_id_survives_u16_max() {
        let frame = make_rtu_request();
        let tcp = rtu_to_tcp(&frame, u16::MAX).unwrap();
        let (_, tid) = tcp_to_rtu(&tcp).unwrap();
        assert_eq!(tid, u16::MAX);
    }

    // ── tcp_resp_to_rtu ───────────────────────────────────────────────────────

    #[test]
    fn tcp_resp_to_rtu_rejects_truncated_pdu() {
        // Header says pdu_len=10 but only 3 bytes follow MBAP.
        let frame: [u8; 9] = [0x00, 0x01, 0x00, 0x00, 0x00, 10, 0x01, 0x03, 0x04];
        assert!(matches!(
            tcp_resp_to_rtu(&frame, 0x0001),
            Err(ModbusError::PayloadTooShort)
        ));
    }

    #[test]
    fn tcp_to_rtu_tid_zero_is_preserved() {
        let frame = make_rtu_request();
        let tcp = rtu_to_tcp(&frame, 0x0000).unwrap();
        let (_, tid) = tcp_to_rtu(&tcp).unwrap();
        assert_eq!(tid, 0x0000);
    }
}
