use std::io::Error as IoError;
use std::error::Error as StdError;
use std::fmt::Display;
use hyper::Error as HyperError;
use serde_json::Error as JsonError;
use websocket::result::WebSocketError;
use byteorder::Error as BoError;

/// Discord API `Result` alias type.
pub type Result<T> = ::std::result::Result<T, Error>;

/// Discord API error type.
#[derive(Debug)]
pub enum Error {
	/// A `hyper` crate error
	Hyper(HyperError),
	/// A `serde_json` crate error
	Json(JsonError),
	/// A `websocket` crate error
	WebSocket(WebSocketError),
	/// A `std::io` module error
	Io(IoError),
	/// A json decoding error, with a description and the offending value
	Decode(&'static str, ::serde_json::Value),
	/// A non-success response from the REST API
	Status(::hyper::status::StatusCode),
	/// An error in the Opus library, with the function name and error code
	Opus(&'static str, i32),
	/// A Discord protocol error, with a description
	Protocol(&'static str),
	/// A miscellaneous error, with a description
	Other(&'static str),
}

impl From<IoError> for Error {
	fn from(err: IoError) -> Error {
		Error::Io(err)
	}
}

impl From<HyperError> for Error {
	fn from(err: HyperError) -> Error {
		Error::Hyper(err)
	}
}

impl From<JsonError> for Error {
	fn from(err: JsonError) -> Error {
		Error::Json(err)
	}
}

impl From<WebSocketError> for Error {
	fn from(err: WebSocketError) -> Error {
		Error::WebSocket(err)
	}
}

impl From<BoError> for Error {
	fn from(err: BoError) -> Error {
		match err {
			BoError::UnexpectedEOF => Error::Other("byteorder::UnexpectedEOF"),
			BoError::Io(io) => Error::Io(io),
		}
	}
}

impl Display for Error {
	fn fmt(&self, f: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
		match *self {
			Error::Hyper(ref inner) => inner.fmt(f),
			Error::Json(ref inner) => inner.fmt(f),
			Error::WebSocket(ref inner) => inner.fmt(f),
			Error::Io(ref inner) => inner.fmt(f),
			_ => f.write_str(self.description()),
		}
	}
}

impl StdError for Error {
	fn description(&self) -> &str {
		match *self {
			Error::Hyper(ref inner) => inner.description(),
			Error::Json(ref inner) => inner.description(),
			Error::WebSocket(ref inner) => inner.description(),
			Error::Io(ref inner) => inner.description(),
			Error::Decode(msg, _) => msg,
			Error::Status(status) => status.canonical_reason().unwrap_or("Unknown bad HTTP status"),
			Error::Opus(msg, _) => msg,
			Error::Protocol(msg) => msg,
			Error::Other(msg) => msg,
		}
	}

	fn cause(&self) -> Option<&StdError> {
		match *self {
			Error::Hyper(ref inner) => Some(inner),
			Error::Json(ref inner) => Some(inner),
			Error::WebSocket(ref inner) => Some(inner),
			Error::Io(ref inner) => Some(inner),
			_ => None,
		}
	}
}
