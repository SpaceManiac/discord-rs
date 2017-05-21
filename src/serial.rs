//! Serde integration support.

use std::fmt;

use serde::*;
use serde::de::{Visitor, Error, Unexpected};

/// Deserialize a maybe-string ID into a u64
pub fn deserialize_id<'d, D: Deserializer<'d>>(d: D) -> Result<u64, D::Error> {
	struct IdVisitor;
	impl<'d> Visitor<'d> for IdVisitor {
		type Value = u64;

		fn expecting(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
			write!(fmt, "a u64 or parseable string")
		}

		fn visit_i64<E: Error>(self, v: i64) -> Result<u64, E> {
			if v >= 0 {
				Ok(v as u64)
			} else {
				Err(E::invalid_value(Unexpected::Signed(v), &self))
			}
		}

		fn visit_u64<E: Error>(self, v: u64) -> Result<u64, E> {
			Ok(v)
		}

		fn visit_str<E: Error>(self, v: &str) -> Result<u64, E> {
			v.parse::<u64>().map_err(|_| E::invalid_value(Unexpected::Str(v), &self))
		}
	}

	d.deserialize_u64(IdVisitor)
}

/// Deserialize a maybe-string discriminator into a u16.
/// Also enforces 0 <= N <= 9999.
#[allow(unused_comparisons)]
pub fn deserialize_discrim<'d, D: Deserializer<'d>>(d: D) -> Result<u16, D::Error> {
    macro_rules! check {
        ($self:ident, $v:ident, $wrong:expr) => {
            if $v >= 0 && $v <= 9999 {
                Ok($v as u16)
            } else {
                Err(E::invalid_value($wrong, &$self))
            }
        }
    }

	struct DiscrimVisitor;
	impl<'d> Visitor<'d> for DiscrimVisitor {
		type Value = u16;

		fn expecting(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
			write!(fmt, "a u16 in [0, 9999] or parseable string")
		}

		fn visit_i64<E: Error>(self, v: i64) -> Result<u16, E> {
            check!(self, v, Unexpected::Signed(v))
		}

		fn visit_u64<E: Error>(self, v: u64) -> Result<u16, E> {
			check!(self, v, Unexpected::Unsigned(v))
		}

		fn visit_str<E: Error>(self, v: &str) -> Result<u16, E> {
			v.parse::<u16>()
                .map_err(|_| E::invalid_value(Unexpected::Str(v), &self))
                .and_then(|v| self.visit_u16(v))
		}
	}

	d.deserialize_u16(DiscrimVisitor)
}

/// Deserialize a single-field struct like a newtype struct.
macro_rules! serial_single_field {
    ($typ:ident as $field:ident: $inner:path) => {
        impl ::serde::Serialize for $typ {
            fn serialize<S: ::serde::ser::Serializer>(&self, s: S) -> ::std::result::Result<S::Ok, S::Error> {
                self.$field.serialize(s)
            }
        }

        impl<'d> ::serde::Deserialize<'d> for $typ {
            fn deserialize<D: ::serde::de::Deserializer<'d>>(d: D) -> ::std::result::Result<$typ, D::Error> {
                <$inner as ::serde::de::Deserialize>::deserialize(d).map(|v| $typ { $field: v })
            }
        }
    }
}
