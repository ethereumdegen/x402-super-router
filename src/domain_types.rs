use primitive_types::U256;
use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct DomainU256(pub U256);

impl DomainU256 {
    pub fn from_string(s: &str) -> Result<Self, String> {
        let cleaned = s.trim();
        if cleaned.starts_with("0x") || cleaned.starts_with("0X") {
            U256::from_str_radix(&cleaned[2..], 16)
                .map(DomainU256)
                .map_err(|e| format!("Failed to parse hex U256: {}", e))
        } else {
            U256::from_dec_str(cleaned)
                .map(DomainU256)
                .map_err(|e| format!("Failed to parse decimal U256: {}", e))
        }
    }
}

impl fmt::Display for DomainU256 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Serialize for DomainU256 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0.to_string())
    }
}

impl<'de> Deserialize<'de> for DomainU256 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct DomainU256Visitor;

        impl<'de> Visitor<'de> for DomainU256Visitor {
            type Value = DomainU256;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a string representing a U256 value")
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                DomainU256::from_string(value).map_err(de::Error::custom)
            }

            fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                self.visit_str(&value)
            }

            fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(DomainU256(U256::from(value)))
            }
        }

        deserializer.deserialize_any(DomainU256Visitor)
    }
}
