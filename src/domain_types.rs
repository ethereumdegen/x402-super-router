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

    /// Parse a human-readable token amount (e.g. "1000" or "1.5") and convert
    /// to raw units by multiplying by 10^decimals.
    pub fn from_human_amount(s: &str, decimals: u8) -> Result<Self, String> {
        let cleaned = s.trim();
        let decimals = decimals as usize;

        let (integer_part, frac_part) = if let Some(dot_pos) = cleaned.find('.') {
            let int_str = &cleaned[..dot_pos];
            let frac_str = cleaned[dot_pos + 1..].trim_end_matches('0');
            if frac_str.len() > decimals {
                return Err(format!(
                    "Too many decimal places: {} has {} but token only has {}",
                    cleaned,
                    frac_str.len(),
                    decimals
                ));
            }
            (int_str, frac_str.to_string())
        } else {
            (cleaned, String::new())
        };

        // Build the full integer string: integer_part + frac_part + padding zeros
        let padding = decimals - frac_part.len();
        let raw_str = format!("{}{}{}", integer_part, frac_part, "0".repeat(padding));

        // Strip leading zeros but keep at least "0"
        let raw_str = raw_str.trim_start_matches('0');
        let raw_str = if raw_str.is_empty() { "0" } else { raw_str };

        U256::from_dec_str(raw_str)
            .map(DomainU256)
            .map_err(|e| format!("Failed to parse human amount '{}': {}", s, e))
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
