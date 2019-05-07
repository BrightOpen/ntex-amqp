use hashbrown::HashMap;
use std::hash::{Hash, Hasher};

use bytes::Bytes;
use chrono::{DateTime, Utc};
use ordered_float::OrderedFloat;
use string::TryFrom;
use uuid::Uuid;

use crate::types::{ByteStr, Descriptor, List, StaticSymbol, Str, Symbol};

/// Represents an AMQP type for use in polymorphic collections
#[derive(Debug, Eq, PartialEq, Hash, Clone, Display, From)]
pub enum Variant {
    /// Indicates an empty value.
    Null,

    /// Represents a true or false value.
    Boolean(bool),

    /// Integer in the range 0 to 2^8 - 1 inclusive.
    Ubyte(u8),

    /// Integer in the range 0 to 2^16 - 1 inclusive.
    Ushort(u16),

    /// Integer in the range 0 to 2^32 - 1 inclusive.
    Uint(u32),

    /// Integer in the range 0 to 2^64 - 1 inclusive.
    Ulong(u64),

    /// Integer in the range 0 to 2^7 - 1 inclusive.
    Byte(i8),

    /// Integer in the range 0 to 2^15 - 1 inclusive.
    Short(i16),

    /// Integer in the range 0 to 2^32 - 1 inclusive.
    Int(i32),

    /// Integer in the range 0 to 2^64 - 1 inclusive.
    Long(i64),

    /// 32-bit floating point number (IEEE 754-2008 binary32).
    Float(OrderedFloat<f32>),

    /// 64-bit floating point number (IEEE 754-2008 binary64).
    Double(OrderedFloat<f64>),

    // Decimal32(d32),
    // Decimal64(d64),
    // Decimal128(d128),
    /// A single Unicode character.
    Char(char),

    /// An absolute point in time.
    /// Represents an approximate point in time using the Unix time encoding of
    /// UTC with a precision of milliseconds. For example, 1311704463521
    /// represents the moment 2011-07-26T18:21:03.521Z.
    Timestamp(DateTime<Utc>),

    /// A universally unique identifier as defined by RFC-4122 section 4.1.2
    Uuid(Uuid),

    /// A sequence of octets.
    #[display(fmt = "Binary({:?})", _0)]
    Binary(Bytes),

    /// A sequence of Unicode characters
    String(Str),

    /// Symbolic values from a constrained domain.
    Symbol(Symbol),

    /// Same as Symbol but for static refs
    StaticSymbol(StaticSymbol),

    /// List
    #[display(fmt = "List({:?})", _0)]
    List(List),

    /// Map
    Map(VariantMap),

    /// Described value
    #[display(fmt = "Described{:?}", _0)]
    Described((Descriptor, Box<Variant>)),
}

impl From<ByteStr> for Variant {
    fn from(s: ByteStr) -> Self {
        Str::from(s).into()
    }
}

impl From<String> for Variant {
    fn from(s: String) -> Self {
        Str::from(string::String::try_from(Bytes::from(s)).unwrap()).into()
    }
}

impl From<&'static str> for Variant {
    fn from(s: &'static str) -> Self {
        Str::from(s).into()
    }
}

impl PartialEq<str> for Variant {
    fn eq(&self, other: &str) -> bool {
        match self {
            Variant::String(s) => s == other,
            Variant::Symbol(s) => s == other,
            _ => false,
        }
    }
}

impl Variant {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Variant::String(s) => Some(s.as_str()),
            Variant::Symbol(s) => Some(s.as_str()),
            _ => None,
        }
    }

    pub fn as_long(&self) -> Option<i64> {
        match self {
            Variant::Ubyte(v) => Some(*v as i64),
            Variant::Ushort(v) => Some(*v as i64),
            Variant::Uint(v) => Some(*v as i64),
            Variant::Ulong(v) => Some(*v as i64),
            Variant::Byte(v) => Some(*v as i64),
            Variant::Short(v) => Some(*v as i64),
            Variant::Int(v) => Some(*v as i64),
            Variant::Long(v) => Some(*v as i64),
            _ => None,
        }
    }

    pub fn as_bytes_str(&self) -> Option<ByteStr> {
        match self {
            Variant::String(s) => Some(s.as_bytes_str()),
            Variant::Symbol(s) => Some(s.as_bytes_str()),
            _ => None,
        }
    }
}

#[derive(PartialEq, Eq, Clone, Debug, Display)]
#[display(fmt = "{:?}", map)]
pub struct VariantMap {
    pub map: HashMap<Variant, Variant>,
}

impl VariantMap {
    pub fn new(map: HashMap<Variant, Variant>) -> VariantMap {
        VariantMap { map }
    }
}

impl Hash for VariantMap {
    fn hash<H: Hasher>(&self, _state: &mut H) {
        unimplemented!()
    }
}

#[derive(PartialEq, Clone, Debug, Display)]
#[display(fmt = "{:?}", _0)]
pub struct VecVariantMap(pub Vec<(Str, Variant)>);

impl Default for VecVariantMap {
    fn default() -> Self {
        VecVariantMap(Vec::with_capacity(8))
    }
}

impl std::ops::Deref for VecVariantMap {
    type Target = Vec<(Str, Variant)>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::ops::DerefMut for VecVariantMap {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_eq() {
        let bytes1 = Variant::Binary(Bytes::from(&b"hello"[..]));
        let bytes2 = Variant::Binary(Bytes::from(&b"hello"[..]));
        let bytes3 = Variant::Binary(Bytes::from(&b"world"[..]));

        assert_eq!(bytes1, bytes2);
        assert!(bytes1 != bytes3);
    }

    #[test]
    fn string_eq() {
        let a = Variant::String(ByteStr::from_str("hello").into());
        let b = Variant::String(ByteStr::from_str("world!").into());

        assert_eq!(Variant::String(ByteStr::from_str("hello").into()), a);
        assert!(a != b);
    }

    #[test]
    fn symbol_eq() {
        let a = Variant::Symbol(Symbol::from("hello"));
        let b = Variant::Symbol(Symbol::from("world!"));

        assert_eq!(Variant::Symbol(Symbol::from("hello")), a);
        assert!(a != b);
    }
}
