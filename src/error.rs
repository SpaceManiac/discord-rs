use std::io::Error as IoError;
use std::error::Error as StdError;
use hyper::Error as HyError;
use serde_json::Error as SjError;
use websocket::result::WebSocketError as WsError;

/// Discord API `Result` alias type.
pub type Result<T> = ::std::result::Result<T, Error>;

/// Discord API error type.
#[derive(Debug)]
pub enum Error {
	Hyper(HyError),
	Json(SjError),
	WebSocket(WsError),
	Io(IoError),
	Decode(&'static str, ::serde_json::Value),
	Status(::hyper::status::StatusCode),
	Other(&'static str),
}

impl From<IoError> for Error {
	fn from(err: IoError) -> Error {
		Error::Io(err)
	}
}

impl From<HyError> for Error {
	fn from(err: HyError) -> Error {
		Error::Hyper(err)
	}
}

impl From<SjError> for Error {
	fn from(err: SjError) -> Error {
		Error::Json(err)
	}
}

impl From<WsError> for Error {
	fn from(err: WsError) -> Error {
		Error::WebSocket(err)
	}
}

impl ::std::fmt::Display for Error {
	fn fmt(&self, f: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
		write!(f, "Discord error ({})", self.description())
	}
}

impl StdError for Error {
	fn description(&self) -> &str {
		match *self {
			Error::Hyper(ref inner) => inner.description(),
			Error::Json(ref inner) => inner.description(),
			Error::WebSocket(ref inner) => inner.description(),
			Error::Io(ref inner) => inner.description(),
			Error::Decode(..) => "json decode error",
			Error::Status(_) => "erroneous HTTP status",
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
