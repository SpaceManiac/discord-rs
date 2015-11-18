use {hyper, serde_json};

pub type Result<T> = ::std::result::Result<T, Error>;

/// Discord API error type.
#[derive(Debug)]
pub enum Error {
	Hyper(hyper::Error),
	Json(serde_json::Error),
	Status(hyper::status::StatusCode),
	Other(&'static str),
}

impl From<hyper::Error> for Error {
	fn from(err: hyper::Error) -> Error {
		Error::Hyper(err)
	}
}

impl From<serde_json::Error> for Error {
	fn from(err: serde_json::Error) -> Error {
		Error::Json(err)
	}
}
