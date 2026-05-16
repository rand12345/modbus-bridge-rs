use core::convert::Infallible;

/// Modbus error type generic over the serial transport error `SE` and TCP transport error `TE`.
///
/// Use `Infallible` for the unused side when only one transport is active (e.g. RTU-only).
/// Call `.into_serial()` / `.into_tcp()` / `.into_combined()` to upcast in flow combinators.
#[derive(Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum ModbusError<SE, TE> {
    Serial(SE),
    Tcp(TE),
    Crc,
    Push,
    PayloadTooShort,
    InvalidTransactionId,
    ConversionSlice,
    RtuIllegal,
}

impl<SE: core::fmt::Debug, TE: core::fmt::Debug> core::fmt::Display for ModbusError<SE, TE> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Serial(e) => write!(f, "serial error: {:?}", e),
            Self::Tcp(e) => write!(f, "TCP error: {:?}", e),
            Self::Crc => write!(f, "CRC mismatch"),
            Self::Push => write!(f, "buffer push error"),
            Self::PayloadTooShort => write!(f, "payload too short"),
            Self::InvalidTransactionId => write!(f, "transaction ID mismatch"),
            Self::ConversionSlice => write!(f, "frame conversion slice error"),
            Self::RtuIllegal => write!(f, "illegal RTU operation"),
        }
    }
}

impl<SE> ModbusError<SE, Infallible> {
    /// Upcast a serial-only error into a combined error by substituting any `TE`.
    #[inline]
    pub fn into_serial<TE>(self) -> ModbusError<SE, TE> {
        match self {
            ModbusError::Serial(se) => ModbusError::Serial(se),
            ModbusError::Tcp(i) => match i {},
            ModbusError::Crc => ModbusError::Crc,
            ModbusError::Push => ModbusError::Push,
            ModbusError::PayloadTooShort => ModbusError::PayloadTooShort,
            ModbusError::InvalidTransactionId => ModbusError::InvalidTransactionId,
            ModbusError::ConversionSlice => ModbusError::ConversionSlice,
            ModbusError::RtuIllegal => ModbusError::RtuIllegal,
        }
    }
}

impl<TE> ModbusError<Infallible, TE> {
    /// Upcast a TCP-only error into a combined error by substituting any `SE`.
    #[inline]
    pub fn into_tcp<SE>(self) -> ModbusError<SE, TE> {
        match self {
            ModbusError::Tcp(te) => ModbusError::Tcp(te),
            ModbusError::Serial(i) => match i {},
            ModbusError::Crc => ModbusError::Crc,
            ModbusError::Push => ModbusError::Push,
            ModbusError::PayloadTooShort => ModbusError::PayloadTooShort,
            ModbusError::InvalidTransactionId => ModbusError::InvalidTransactionId,
            ModbusError::ConversionSlice => ModbusError::ConversionSlice,
            ModbusError::RtuIllegal => ModbusError::RtuIllegal,
        }
    }
}

impl ModbusError<Infallible, Infallible> {
    /// Upcast a framing-only error into any combined error type.
    #[inline]
    pub fn into_combined<SE, TE>(self) -> ModbusError<SE, TE> {
        match self {
            ModbusError::Serial(i) | ModbusError::Tcp(i) => match i {},
            ModbusError::Crc => ModbusError::Crc,
            ModbusError::Push => ModbusError::Push,
            ModbusError::PayloadTooShort => ModbusError::PayloadTooShort,
            ModbusError::InvalidTransactionId => ModbusError::InvalidTransactionId,
            ModbusError::ConversionSlice => ModbusError::ConversionSlice,
            ModbusError::RtuIllegal => ModbusError::RtuIllegal,
        }
    }
}
