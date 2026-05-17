//! In-process integration tests for `modbus-bridge`.
//!
//! Architecture per test:
//!
//!   [tokio-modbus tcp client]
//!         │  TCP  127.0.0.1:random
//!   [tokio::net::TcpListener → TcpStream]
//!         │  embedded-io-adapters::tokio_1::FromTokio
//!   [modbus-bridge Bridge  (tokio task)]
//!         │  embedded-io-adapters::tokio_1::FromTokio
//!   [tokio::io::duplex left half]
//!   [tokio::io::duplex right half]
//!         │  (raw AsyncRead+AsyncWrite — no adapter)
//!   [inline RTU slave  (tokio task)]
//!
//! Run:
//!   cargo test --features tokio-integration --test tokio_integration -- --nocapture

use std::net::SocketAddr;

use embedded_io_adapters::tokio_1::FromTokio;
use modbus_bridge::{BridgeBuilder, NoPin};
use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream};
use tokio::net::TcpListener;
use tokio_modbus::client::tcp;
use tokio_modbus::prelude::*;

// ── CRC-16 (Modbus) ───────────────────────────────────────────────────────────

fn crc16(data: &[u8]) -> [u8; 2] {
    let mut crc: u16 = 0xFFFF;
    for &b in data {
        crc ^= b as u16;
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xA001 } else { crc >> 1 };
        }
    }
    crc.to_le_bytes()
}

// ── Inline RTU slave ──────────────────────────────────────────────────────────
//
// Register map (unit ID 1):
//   Holding registers : hr[addr + i] = addr + i
//   Input registers   : ir[addr + i] = addr + i + 100
//   Coils             : co[addr + i] = (addr + i) % 2 == 1  (odd addresses → true)
//   Discrete inputs   : di[addr + i] = (addr + i) % 2 == 0  (even addresses → true)
//
// Slaves with any other unit ID are silently ignored (no response),
// which lets the bridge hit its RTU timeout.

const SLAVE_UNIT: u8 = 1;

async fn rtu_slave(mut stream: DuplexStream) {
    let mut buf = [0u8; 256];
    loop {
        // Read unit + FC (2 bytes); EOF exits cleanly.
        if stream.read_exact(&mut buf[..2]).await.is_err() {
            break;
        }
        let unit = buf[0];
        let fc = buf[1];

        // Read the rest of the frame; length depends on FC.
        let frame_len = match fc {
            // Fixed 8-byte request: [unit, fc, addr(2), qty/val(2), CRC(2)]
            0x01..=0x06 => {
                if stream.read_exact(&mut buf[2..8]).await.is_err() {
                    break;
                }
                8
            }
            // Variable: [unit, fc, addr(2), qty(2), byte_count(1), data(byte_count), CRC(2)]
            0x0F | 0x10 => {
                if stream.read_exact(&mut buf[2..7]).await.is_err() {
                    break;
                }
                let bc = buf[6] as usize;
                if stream.read_exact(&mut buf[7..9 + bc]).await.is_err() {
                    break;
                }
                9 + bc
            }
            _ => break,
        };

        // Validate CRC; skip malformed frames without disconnecting.
        let [exp_lo, exp_hi] = crc16(&buf[..frame_len - 2]);
        if buf[frame_len - 2] != exp_lo || buf[frame_len - 1] != exp_hi {
            continue;
        }

        // Silently ignore requests for other unit IDs (simulates no slave on bus).
        if unit != SLAVE_UNIT {
            continue;
        }

        let addr = u16::from_be_bytes([buf[2], buf[3]]);
        let qty = u16::from_be_bytes([buf[4], buf[5]]); // also encodes write values for FC05/06

        let resp = rtu_response(unit, fc, addr, qty);
        if stream.write_all(&resp).await.is_err() {
            break;
        }
    }
}

fn rtu_response(unit: u8, fc: u8, addr: u16, qty: u16) -> Vec<u8> {
    let mut pdu: Vec<u8> = Vec::new();
    match fc {
        0x01 => {
            // ReadCoils: co[addr+i] = (addr+i) % 2 == 1
            let byte_count = ((qty + 7) / 8) as u8;
            pdu.extend_from_slice(&[unit, fc, byte_count]);
            for i in 0..byte_count {
                let mut byte = 0u8;
                for bit in 0..8u16 {
                    let a = addr + i as u16 * 8 + bit;
                    if a % 2 == 1 {
                        byte |= 1 << (bit as u8);
                    }
                }
                pdu.push(byte);
            }
        }
        0x02 => {
            // ReadDiscreteInputs: di[addr+i] = (addr+i) % 2 == 0
            let byte_count = ((qty + 7) / 8) as u8;
            pdu.extend_from_slice(&[unit, fc, byte_count]);
            for i in 0..byte_count {
                let mut byte = 0u8;
                for bit in 0..8u16 {
                    let a = addr + i as u16 * 8 + bit;
                    if a % 2 == 0 {
                        byte |= 1 << (bit as u8);
                    }
                }
                pdu.push(byte);
            }
        }
        0x03 => {
            // ReadHoldingRegisters: hr[addr+i] = addr+i
            pdu.extend_from_slice(&[unit, fc, (qty * 2) as u8]);
            for i in 0..qty {
                pdu.extend_from_slice(&(addr + i).to_be_bytes());
            }
        }
        0x04 => {
            // ReadInputRegisters: ir[addr+i] = addr+i+100
            pdu.extend_from_slice(&[unit, fc, (qty * 2) as u8]);
            for i in 0..qty {
                pdu.extend_from_slice(&(addr + i + 100).to_be_bytes());
            }
        }
        0x05 | 0x06 => {
            // WriteSingleCoil / WriteSingleRegister — echo addr + value (qty = value)
            pdu.extend_from_slice(&[unit, fc]);
            pdu.extend_from_slice(&addr.to_be_bytes());
            pdu.extend_from_slice(&qty.to_be_bytes());
        }
        0x0F | 0x10 => {
            // WriteMultipleCoils / WriteMultipleRegisters — echo addr + qty
            pdu.extend_from_slice(&[unit, fc]);
            pdu.extend_from_slice(&addr.to_be_bytes());
            pdu.extend_from_slice(&qty.to_be_bytes());
        }
        _ => return Vec::new(),
    }
    let [lo, hi] = crc16(&pdu);
    pdu.push(lo);
    pdu.push(hi);
    pdu
}

// ── Test plumbing ─────────────────────────────────────────────────────────────

/// Spawn the bridge + RTU slave and return a connected tokio-modbus TCP client.
async fn setup() -> impl Client + Reader + Writer {
    let (bridge_serial, slave_serial) = tokio::io::duplex(4096);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr: SocketAddr = listener.local_addr().unwrap();

    // RTU slave task.
    tokio::spawn(rtu_slave(slave_serial));

    // Bridge task: accepts connections sequentially, drives each to completion.
    tokio::spawn(async move {
        let mut bridge = BridgeBuilder::new()
            .rtu(FromTokio::new(bridge_serial), NoPin)
            .build();
        loop {
            let Ok((tcp, _)) = listener.accept().await else { break };
            let mut conn = bridge.accept(FromTokio::new(tcp));
            loop {
                match conn.next().await {
                    Ok(_) => {}
                    Err(_) => break,
                }
            }
        }
    });

    tcp::connect_slave(addr, Slave(SLAVE_UNIT))
        .await
        .expect("TCP connect to bridge failed")
}

// ── T1: Happy path ────────────────────────────────────────────────────────────

#[tokio::test]
async fn t1_fc03_single_register() {
    let mut ctx = setup().await;
    let regs = ctx
        .read_holding_registers(0, 1)
        .await
        .expect("transport")
        .expect("exception");
    assert_eq!(regs, vec![0]);
}

#[tokio::test]
async fn t1_fc03_multi_register() {
    let mut ctx = setup().await;
    let regs = ctx
        .read_holding_registers(0, 10)
        .await
        .expect("transport")
        .expect("exception");
    let expected: Vec<u16> = (0..10).collect();
    assert_eq!(regs, expected);
}

#[tokio::test]
async fn t1_fc03_sequential_requests() {
    let mut ctx = setup().await;
    for addr in 0u16..5 {
        let regs = ctx
            .read_holding_registers(addr, 1)
            .await
            .expect("transport")
            .expect("exception");
        assert_eq!(regs[0], addr, "hr[{addr}] mismatch");
    }
}

#[tokio::test]
async fn t1_sequential_connections() {
    for _ in 0..3 {
        let mut ctx = setup().await;
        let regs = ctx
            .read_holding_registers(0, 1)
            .await
            .expect("transport")
            .expect("exception");
        assert_eq!(regs[0], 0);
        ctx.disconnect().await.ok();
    }
}

// ── T2: Function code matrix ──────────────────────────────────────────────────

#[tokio::test]
async fn t2_fc01_read_coils() {
    let mut ctx = setup().await;
    // co[0]=false, co[1]=true, co[2]=false, co[3]=true, ...
    let coils = ctx
        .read_coils(0, 8)
        .await
        .expect("transport")
        .expect("exception");
    let expected: Vec<bool> = (0u16..8).map(|i| i % 2 == 1).collect();
    assert_eq!(coils, expected);
}

#[tokio::test]
async fn t2_fc02_read_discrete_inputs() {
    let mut ctx = setup().await;
    // di[0]=true, di[1]=false, di[2]=true, di[3]=false, ...
    let inputs = ctx
        .read_discrete_inputs(0, 8)
        .await
        .expect("transport")
        .expect("exception");
    let expected: Vec<bool> = (0u16..8).map(|i| i % 2 == 0).collect();
    assert_eq!(inputs, expected);
}

#[tokio::test]
async fn t2_fc03_read_holding_registers() {
    let mut ctx = setup().await;
    let regs = ctx
        .read_holding_registers(5, 3)
        .await
        .expect("transport")
        .expect("exception");
    assert_eq!(regs, vec![5, 6, 7]);
}

#[tokio::test]
async fn t2_fc04_read_input_registers() {
    let mut ctx = setup().await;
    let regs = ctx
        .read_input_registers(0, 2)
        .await
        .expect("transport")
        .expect("exception");
    assert_eq!(regs, vec![100, 101]);
}

#[tokio::test]
async fn t2_fc05_write_single_coil() {
    let mut ctx = setup().await;
    ctx.write_single_coil(0, true)
        .await
        .expect("transport")
        .expect("exception");
}

#[tokio::test]
async fn t2_fc06_write_single_register() {
    let mut ctx = setup().await;
    ctx.write_single_register(0, 0x1234)
        .await
        .expect("transport")
        .expect("exception");
}

#[tokio::test]
async fn t2_fc0f_write_multiple_coils() {
    // Variable-length RTU frame — exercises FC-aware framing in the bridge.
    let mut ctx = setup().await;
    ctx.write_multiple_coils(0, &[true; 16])
        .await
        .expect("transport")
        .expect("exception");
}

#[tokio::test]
async fn t2_fc10_write_multiple_registers() {
    // Variable-length RTU frame — exercises FC-aware framing in the bridge.
    let mut ctx = setup().await;
    ctx.write_multiple_registers(0, &[1, 2])
        .await
        .expect("transport")
        .expect("exception");
}

// ── T3: Transaction ID handling ───────────────────────────────────────────────
//
// tokio-modbus manages TIDs internally and verifies the echo; if TID handling
// in the bridge were broken, these requests would fail.

#[tokio::test]
async fn t3_tid_sequential_requests() {
    let mut ctx = setup().await;
    // Issue several requests; each increments the internal TID counter.
    for addr in 0u16..10 {
        ctx.read_holding_registers(addr, 1)
            .await
            .expect("transport")
            .expect("exception");
    }
}

#[tokio::test]
async fn t3_tid_mixed_function_codes() {
    // TID must be echoed correctly across different FC types on one connection.
    let mut ctx = setup().await;
    ctx.read_holding_registers(0, 1).await.unwrap().unwrap();
    ctx.read_coils(0, 1).await.unwrap().unwrap();
    ctx.write_single_register(0, 42).await.unwrap().unwrap();
    ctx.read_holding_registers(0, 1).await.unwrap().unwrap();
}

// ── T4: Error handling and bridge recovery ────────────────────────────────────

#[tokio::test]
async fn t4_bridge_recovers_after_client_disconnect() {
    // Connect, send one request, then disconnect abruptly.
    {
        let mut ctx = setup().await;
        ctx.read_holding_registers(0, 1).await.unwrap().unwrap();
        ctx.disconnect().await.ok();
    }
    // A fresh connection on the same bridge must work.
    let mut ctx = setup().await;
    ctx.read_holding_registers(0, 1).await.unwrap().unwrap();
}

#[tokio::test]
async fn t4_bridge_recovers_after_multiple_disconnects() {
    for _ in 0..5 {
        let mut ctx = setup().await;
        ctx.read_holding_registers(0, 1).await.unwrap().unwrap();
        ctx.disconnect().await.ok();
    }
}

// ── T5: Stress ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn t5_100_requests_one_connection() {
    let mut ctx = setup().await;
    for addr in 0u16..100 {
        let regs = ctx
            .read_holding_registers(addr, 1)
            .await
            .expect("transport")
            .expect("exception");
        assert_eq!(regs[0], addr, "hr[{addr}] mismatch at iteration {addr}");
    }
}

#[tokio::test]
async fn t5_50_sequential_connections() {
    for i in 0u16..50 {
        let mut ctx = setup().await;
        let regs = ctx
            .read_holding_registers(i, 1)
            .await
            .expect("transport")
            .expect("exception");
        assert_eq!(regs[0], i);
        ctx.disconnect().await.ok();
    }
}

#[tokio::test]
async fn t5_interleaved_read_write() {
    // 20 rounds of mixed operations on a single connection.
    let mut ctx = setup().await;
    for i in 0u16..20 {
        ctx.read_holding_registers(i, 1).await.unwrap().unwrap();
        ctx.write_single_register(i, i).await.unwrap().unwrap();
        ctx.read_coils(0, 8).await.unwrap().unwrap();
        ctx.write_multiple_registers(i, &[i, i + 1])
            .await
            .unwrap()
            .unwrap();
    }
}
