use opus_sys as opus;

use super::{Error, Result};

// Opus encoding wrapper

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
		let len = unsafe { opus::opus_encode(self.0, input.as_ptr(), input.len() as i32, output.as_mut_ptr(), output.len() as i32) };
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

// Opus
