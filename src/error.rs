use std::io::Error as IoError;
use std::error::Error as StdError;
use std::fmt::Display;
use hyper::Error as HyperError;
use serde_json::Error as JsonError;
use serde_json::Value;
use websocket::result::WebSocketError;
#[cfg(feature="voice")]
use opus::Error as OpusError;
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
	/// An error in the Opus library, with the function name and error code
	#[cfg(feature="voice")]
	Opus(OpusError),
	/// A websocket connection was closed, possibly with a message
	Closed(Option<u16>, String),
	/// A json decoding error, with a description and the offending value
	Decode(&'static str, Value),
	/// A generic non-success response from the REST API
	Status(::hyper::status::StatusCode, Option<Value>),
	/// A rate limit error, with how many milliseconds to wait before retrying
	RateLimited(u64),
	/// A Discord protocol error, with a description
	Protocol(&'static str),
	/// A command execution failure, with a command name and output
	Command(&'static str, ::std::process::Output),
	/// A miscellaneous error, with a description
	Other(&'static str),
}

impl Error {
	#[doc(hidden)]
	pub fn from_response(response: ::hyper::client::Response) -> Error {
		let status = response.status;
		let value = ::serde_json::from_reader(response).ok();
		if status == ::hyper::status::StatusCode::TooManyRequests {
			if let Some(Value::Object(ref map)) = value {
				if let Some(delay) = map.get("retry_after").and_then(|v| v.as_u64()) {
					return Error::RateLimited(delay)
				}
			}
		}
		Error::Status(status, value)
	}
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

#[cfg(feature="voice")]
impl From<OpusError> for Error {
	fn from(err: OpusError) -> Error {
		Error::Opus(err)
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
			#[cfg(feature="voice")]
			Error::Opus(ref inner) => inner.fmt(f),
			Error::Command(ref cmd, _) => write!(f, "Command failed: {}", cmd),
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
			#[cfg(feature="voice")]
			Error::Opus(ref inner) => inner.description(),
			Error::Closed(_, _) => "Connection closed",
			Error::Decode(msg, _) => msg,
			Error::Status(status, _) => status.canonical_reason().unwrap_or("Unknown bad HTTP status"),
			Error::RateLimited(_) => "Rate limited",
			Error::Protocol(msg) => msg,
			Error::Command(_, _) => "Command failed",
			Error::Other(msg) => msg,
		}
	}

	fn cause(&self) -> Option<&StdError> {
		match *self {
			Error::Hyper(ref inner) => Some(inner),
			Error::Json(ref inner) => Some(inner),
			Error::WebSocket(ref inner) => Some(inner),
			Error::Io(ref inner) => Some(inner),
			#[cfg(feature="voice")]
			Error::Opus(ref inner) => Some(inner),
			_ => None,
		}
	}
}
