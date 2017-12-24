//! Serde integration support.

use std::fmt;
use std::marker::PhantomData;

use serde::*;
use serde::de::{Visitor, Error, Unexpected};

fn i64_to_u64<'d, V: Visitor<'d>, E: Error>(v: V, n: i64) -> Result<V::Value, E> {
	if n >= 0 {
		v.visit_u64(n as u64)
	} else {
		Err(E::invalid_value(Unexpected::Signed(n), &v))
	}
}

/// Ignore deserialization errors and revert to default.
pub fn ignore_errors<'d, T: Deserialize<'d> + Default, D: Deserializer<'d>>(d: D) -> Result<T, D::Error> {
	use serde_json::Value;
	
	let v = Value::deserialize(d)?;
   	Ok(T::deserialize(v).ok().unwrap_or_default())
}

/// Deserialize a maybe-string ID into a u64.
pub fn deserialize_id<'d, D: Deserializer<'d>>(d: D) -> Result<u64, D::Error> {
	struct IdVisitor;
	impl<'d> Visitor<'d> for IdVisitor {
		type Value = u64;

		fn expecting(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
			write!(fmt, "a u64 or parseable string")
		}

		fn visit_i64<E: Error>(self, v: i64) -> Result<u64, E> {
			i64_to_u64(self, v)
		}

		fn visit_u64<E: Error>(self, v: u64) -> Result<u64, E> {
			Ok(v)
		}

		fn visit_str<E: Error>(self, v: &str) -> Result<u64, E> {
			v.parse::<u64>().map_err(|_| E::invalid_value(Unexpected::Str(v), &self))
		}
	}

	d.deserialize_any(IdVisitor)
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

	d.deserialize_any(DiscrimVisitor)
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

/// Special support for the oddly complex `ReactionEmoji`.
pub mod reaction_emoji {
	use super::*;
	use model::{ReactionEmoji, EmojiId};

	#[derive(Serialize)]
	struct EmojiSer<'s> {
		name: &'s str,
		id: Option<EmojiId>
	}

	#[derive(Deserialize)]
	struct EmojiDe {
		name: String,
		id: Option<EmojiId>,
	}

	pub fn serialize<S: Serializer>(v: &ReactionEmoji, s: S) -> Result<S::Ok, S::Error> {
		(match *v {
			ReactionEmoji::Unicode(ref name) => EmojiSer { name: name, id: None },
			ReactionEmoji::Custom { ref name, id } => EmojiSer { name: name, id: Some(id) },
		}).serialize(s)
	}

	pub fn deserialize<'d, D: Deserializer<'d>>(d: D) -> Result<ReactionEmoji, D::Error> {
		Ok(match try!(EmojiDe::deserialize(d)) {
			EmojiDe { name, id: None } => ReactionEmoji::Unicode(name),
			EmojiDe { name, id: Some(id) } => ReactionEmoji::Custom { name: name, id: id },
		})
	}
}

/// Support for named enums.
pub mod named {
	use super::*;

	pub trait NamedEnum: Sized {
		fn name(&self) -> &'static str;
		fn from_name(name: &str) -> Option<Self>;
		fn typename() -> &'static str;
	}

	pub fn serialize<T: NamedEnum, S: Serializer>(v: &T, s: S) -> Result<S::Ok, S::Error> {
		v.name().serialize(s)
	}

	pub fn deserialize<'d, T: NamedEnum, D: Deserializer<'d>>(d: D) -> Result<T, D::Error> {
		struct NameVisitor<T>(PhantomData<T>);
		impl<'d, T: NamedEnum> Visitor<'d> for NameVisitor<T> {
			type Value = T;

			fn expecting(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
				write!(fmt, "a valid {} name", T::typename())
			}

			fn visit_str<E: Error>(self, v: &str) -> Result<T, E> {
				T::from_name(v).ok_or_else(|| E::invalid_value(Unexpected::Str(v), &self))
			}
		}

		d.deserialize_string(NameVisitor(PhantomData))
	}
}
macro_rules! serial_names {
	($typ:ident; $($entry:ident, $value:expr;)*) => {
		impl $typ {
			pub fn name(&self) -> &'static str {
				match *self {
					$($typ::$entry => $value,)*
				}
			}

			pub fn from_name(name: &str) -> Option<Self> {
				match name {
					$($value => Some($typ::$entry),)*
					_ => None,
				}
			}
		}

		impl ::serial::named::NamedEnum for $typ {
			fn name(&self) -> &'static str {
				self.name()
			}

			fn from_name(name: &str) -> Option<Self> {
				Self::from_name(name)
			}

			fn typename() -> &'static str {
				stringify!($typ)
			}
		}
	}
}

/// Support for numeric enums.
pub mod numeric {
	use super::*;

	pub trait NumericEnum: Sized {
		fn num(&self) -> u64;
		fn from_num(num: u64) -> Option<Self>;
		fn typename() -> &'static str;
	}

	pub fn serialize<T: NumericEnum, S: Serializer>(v: &T, s: S) -> Result<S::Ok, S::Error> {
		v.num().serialize(s)
	}

	pub fn deserialize<'d, T: NumericEnum, D: Deserializer<'d>>(d: D) -> Result<T, D::Error> {
		struct NumVisitor<T>(PhantomData<T>);
		impl<'d, T: NumericEnum> Visitor<'d> for NumVisitor<T> {
			type Value = T;

			fn expecting(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
				write!(fmt, "a valid {} number", T::typename())
			}

			fn visit_i64<E: Error>(self, v: i64) -> Result<T, E> {
				i64_to_u64(self, v)
			}

			fn visit_u64<E: Error>(self, v: u64) -> Result<T, E> {
				T::from_num(v).ok_or_else(|| E::invalid_value(Unexpected::Unsigned(v), &self))
			}
		}

		d.deserialize_any(NumVisitor(PhantomData))
	}
}
macro_rules! serial_numbers {
	($typ:ident; $($entry:ident, $value:expr;)*) => {
		impl $typ {
			pub fn num(&self) -> u64 {
				match *self {
					$($typ::$entry => $value,)*
				}
			}

			pub fn from_num(num: u64) -> Option<Self> {
				match num {
					$($value => Some($typ::$entry),)*
					_ => None,
				}
			}
		}
		impl ::serial::numeric::NumericEnum for $typ {
			fn num(&self) -> u64 {
				self.num()
			}

			fn from_num(num: u64) -> Option<Self> {
				Self::from_num(num)
			}

			fn typename() -> &'static str {
				stringify!($typ)
			}
		}
	}
}

/// Support for using "named" or "numeric" as the default ser/de impl.
macro_rules! serial_use_mapping {
	($typ:ident, $which:ident) => {
		impl ::serde::Serialize for $typ {
			#[inline]
			fn serialize<S: ::serde::ser::Serializer>(&self, s: S) -> ::std::result::Result<S::Ok, S::Error> {
				::serial::$which::serialize(self, s)
			}
		}

		impl<'d> ::serde::Deserialize<'d> for $typ {
			#[inline]
			fn deserialize<D: ::serde::de::Deserializer<'d>>(d: D) -> ::std::result::Result<$typ, D::Error> {
				::serial::$which::deserialize(d)
			}
		}
	}
}
