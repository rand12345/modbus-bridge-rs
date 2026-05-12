//! Typestate builder for [`Client`](crate::Client).

pub struct ClientBuilder<S, TX, D = crate::NoDelay> {
    pub(crate) serial: S,
    pub(crate) tx_en: TX,
    pub(crate) rtu_timeout_ms: Option<u32>,
    pub(crate) tcp_timeout_ms: Option<u32>,
    pub(crate) delay: D,
}
