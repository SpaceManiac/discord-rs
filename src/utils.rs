use opus_sys as opus;
use time::{Duration, Timespec, get_time};

use super::{Error, Result};

// Safe wrapper for an OpusEncoder
// TODO: support stereo

//const OPUS_APPLICATION_VOIP: i32 = 2048;
const OPUS_APPLICATION_AUDIO: i32 = 2049;
//const OPUS_APPLICATION_RESTRICTED_LOWDELAY: i32 = 2051;

pub struct OpusEncoder(*mut opus::OpusEncoder);

impl OpusEncoder {
	pub fn new() -> Result<OpusEncoder> {
		let mut error = 0;
		let ptr = unsafe { opus::opus_encoder_create(48000, 1, OPUS_APPLICATION_AUDIO, &mut error) };
		if error != opus::OPUS_OK || ptr.is_null() {
			Err(Error::Opus("opus_encoder_create", error))
		} else {
			Ok(OpusEncoder(ptr))
		}
	}

	pub fn encode(&mut self, input: &[i16], output: &mut [u8]) -> Result<usize> {
		let len = unsafe { opus::opus_encode(self.0,
			input.as_ptr(), input.len() as i32,
			output.as_mut_ptr(), output.len() as i32) };
		if len < 0 {
			Err(Error::Opus("opus_encode", len))
		} else {
			Ok(len as usize)
		}
	}
}

impl Drop for OpusEncoder {
	fn drop(&mut self) {
		unsafe { opus::opus_encoder_destroy(self.0) }
	}
}

// Timer that remembers when it is supposed to go off
pub struct Timer(Timespec);

impl Timer {
	pub fn new(initial_delay: Duration) -> Timer {
		Timer(get_time() + initial_delay)
	}

	pub fn immediately(&mut self) {
		self.0 = get_time();
	}

	pub fn check_and_add(&mut self, duration: Duration) -> bool {
		if get_time() >= self.0 {
			self.0 = self.0 + duration;
			true
		} else { false }
	}
}
