//! [`Client`] — owns the RTU serial port and creates [`ClientSession`]s.

pub struct Client<S, TX, D = crate::NoDelay> {
    pub(crate) rtu: crate::rtu::ModbusRtu<S, TX>,
    pub(crate) rtu_timeout_ms: Option<u32>,
    pub(crate) tcp_timeout_ms: Option<u32>,
    pub(crate) delay: D,
}
