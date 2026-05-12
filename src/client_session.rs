//! [`ClientSession`] — a live TCP session bound to a [`Client`](crate::Client).

pub struct ClientSession<'b, S, TX, TS, D = crate::NoDelay> {
    pub(crate) client: &'b mut crate::client::Client<S, TX, D>,
    pub(crate) tcp: crate::tcp::ModbusTcp<TS>,
}
