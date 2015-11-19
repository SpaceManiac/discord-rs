use std::io::Error as IoError;
use hyper::Error as HyError;
use serde_json::Error as SjError;
use websocket::result::WebSocketError as WsError;

pub type Result<T> = ::std::result::Result<T, Error>;

/// Discord API error type.
#[derive(Debug)]
pub enum Error {
	Hyper(HyError),
	Json(SjError),
	WebSocket(WsError),
	Io(IoError),
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
