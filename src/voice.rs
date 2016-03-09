//! Voice communication module.

use super::{Result, Error};

use std::io::{self, Read, Write};
use std::sync::mpsc;
use std::net::UdpSocket;

use websocket::ws::sender::Sender as SenderTrait;
use websocket::client::{Client, Sender, Receiver};
use websocket::stream::WebSocketStream;
use websocket::message::{Message as WsMessage, Type as MessageType};

use serde_json;
use serde_json::builder::ObjectBuilder;

use byteorder::{LittleEndian, BigEndian, WriteBytesExt, ReadBytesExt};

use super::model::*;

/// A readable audio source.
///
/// Audio is expected to be in signed 16-bit little-endian PCM (`pcm_s16le`)
/// format, at 48000Hz.
pub type AudioSource = Box<Read + Send>;

/// A websocket connection to the voice servers.
///
/// A VoiceConnection may be active or inactive. Use `voice_connect` and
/// `voice_disconnect` on the `Connection` you are feeding it events from to
/// change what channel it is connected to.
pub struct VoiceConnection {
	user_id: UserId,
	session_id: Option<String>,
	sender: mpsc::Sender<Status>,
	receiver: Option<mpsc::Receiver<Status>>,
}

/// A receiver for incoming audio.
pub trait AudioReceiver: Send {
	/// Called when a user's currently-speaking state has updated.
	///
	/// This method is the only way to know the `ssrc` to `user_id` mapping.
	fn speaking_update(&mut self, ssrc: u32, user_id: &UserId, speaking: bool);

	/// Called when a voice packet is received.
	///
	/// The sequence number and timestamp usually increase regularly over time, but packets may
	/// be received out of order depending on network conditions. The length of the `data` slice
	/// will be 960 for mono audio and twice that for stereo audio (with samples interleaved).
	fn voice_packet(&mut self, ssrc: u32, sequence: u16, timestamp: u32, data: &[i16]);
}

impl VoiceConnection {
	/// Prepare a VoiceConnection for later use.
	pub fn new(user_id: UserId) -> Self {
		let (tx, rx) = mpsc::channel();
		VoiceConnection {
			user_id: user_id,
			session_id: None,
			sender: tx,
			receiver: Some(rx),
		}
	}

	/// Play from the given audio source.
	pub fn play(&self, source: AudioSource) {
		let _ = self.sender.send(Status::SetSource(Some(source)));
	}

	/// Stop the currently playing audio source.
	pub fn stop(&self) {
		let _ = self.sender.send(Status::SetSource(None));
	}

	/// Set the receiver to which incoming voice will be sent.
	pub fn set_receiver(&self, receiver: Box<AudioReceiver>) {
		let _ = self.sender.send(Status::SetReceiver(Some(receiver)));
	}

	/// Clear the voice receiver, discarding incoming voice.
	pub fn clear_receiver(&self) {
		let _ = self.sender.send(Status::SetReceiver(None));
	}

	/// Update the voice state based on an event.
	pub fn update(&mut self, event: &Event) {
		match *event {
			Event::VoiceStateUpdate(_, ref voice_state) => {
				if voice_state.user_id == self.user_id {
					self.session_id = Some(voice_state.session_id.clone());
					if !voice_state.channel_id.is_some() {
						// drop the previous connection
						self.disconnect();
					}
				}
			}
			Event::VoiceServerUpdate { ref server_id, ref endpoint, ref token } => {
				if let Some(endpoint) = endpoint.as_ref() {
					self.connect(server_id, endpoint.clone(), token).expect("Voice::connect failure")
				} else {
					self.disconnect()
				}
			}
			_ => {}
		}
	}

	/// Check whether the voice thread is currently running.
	pub fn is_running(&self) -> bool {
		match self.receiver {
			None => self.sender.send(Status::Poke).is_ok(),
			Some(_) => false,
		}
	}

	fn disconnect(&mut self) {
		let (tx, rx) = mpsc::channel();
		self.sender = tx;
		self.receiver = Some(rx);
	}

	fn connect(&mut self, server_id: &ServerId, mut endpoint: String, token: &str) -> Result<()> {
		// take any pending receiver, or build a new one if there isn't any
		let rx = match self.receiver.take() {
			Some(rx) => rx,
			None => {
				let (tx, rx) = mpsc::channel();
				self.sender = tx;
				rx
			}
		};

		// prepare the URL: drop the :80 and prepend wss://
		if endpoint.ends_with(":80") {
			let len = endpoint.len();
			endpoint.truncate(len - 3);
		}
		// establish the websocket connection
		let url = match ::websocket::client::request::Url::parse(&format!("wss://{}", endpoint)) {
			Ok(url) => url,
			Err(_) => return Err(Error::Other("Invalid URL in Voice::connect()"))
		};
		let response = try!(try!(Client::connect(url)).send());
		try!(response.validate());
		let (mut sender, receiver) = response.begin().split();

		// send the handshake
		let map = ObjectBuilder::new()
			.insert("op", 0)
			.insert_object("d", |object| object
				.insert("server_id", &server_id.0)
				.insert("user_id", &self.user_id.0)
				.insert("session_id", self.session_id.as_ref().expect("no session id"))
				.insert("token", token)
			)
			.unwrap();
		try!(sender.send_message(&WsMessage::text(try!(serde_json::to_string(&map)))));

		// spin up the voice thread, where most of the action will take place
		try!(::std::thread::Builder::new()
			.name("Discord Voice Thread".into())
			.spawn(move || voice_thread(endpoint, sender, receiver, rx).unwrap()));
		Ok(())
	}
}

/// Use `ffmpeg` to open an audio file as a PCM stream.
///
/// Requires `ffmpeg` to be on the path and executable.
pub fn open_ffmpeg_stream<P: AsRef<::std::ffi::OsStr>>(path: P) -> Result<AudioSource> {
	use std::process::{Command, Stdio};
	let child = try!(Command::new("ffmpeg")
		.arg("-i").arg(path)
		.args(&[
			"-f", "s16le",
			"-ac", "1",
			"-ar", "48000",
			"-acodec", "pcm_s16le",
			"-"])
		.stdin(Stdio::null())
		.stdout(Stdio::piped())
		.stderr(Stdio::null())
		.spawn());
	Ok(Box::new(ProcessStream(child)))
}

/// Use `youtube-dl` and `ffmpeg` to stream from an internet source.
///
/// Requires both `youtube-dl` and `ffmpeg` to be on the path and executable.
/// On Windows, this means the `.exe` version of `youtube-dl` must be used.
pub fn open_ytdl_stream(url: &str) -> Result<AudioSource> {
	use std::process::{Command, Stdio};
	let output = try!(Command::new("youtube-dl")
		.args(&[
			"-f", "webm[abr>0]/bestaudio/best",
			"--no-playlist", "--print-json",
			"--skip-download",
			url])
		.stdin(Stdio::null())
		.output());
	if !output.status.success() {
		return Err(Error::Other("youtube-dl failed"))
	}

	let json: serde_json::Value = try!(serde_json::from_reader(&output.stdout[..]));
	let map = match json.as_object() {
		Some(map) => map,
		None => return Err(Error::Other("youtube-dl output could not be read"))
	};
	let url = match map.get("url").and_then(serde_json::Value::as_string) {
		Some(url) => url,
		None => return Err(Error::Other("youtube-dl output's \"url\" could not be read"))
	};
	open_ffmpeg_stream(url)
}

/// A stream that reads from a child's stdout and kills it on drop.
struct ProcessStream(::std::process::Child);

impl Read for ProcessStream {
	fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
		self.0.stdout.as_mut().expect("missing stdout").read(buf)
	}
}

impl Drop for ProcessStream {
	fn drop(&mut self) {
		// If we can't kill it, it's dead already or out of our hands
		let _ = self.0.kill();
	}
}

enum Status {
	SetSource(Option<AudioSource>),
	SetReceiver(Option<Box<AudioReceiver>>),
	Poke,
}

enum RecvStatus {
	Websocket(VoiceEvent),
	Udp(Vec<u8>),
}

fn recv_message(receiver: &mut Receiver<WebSocketStream>) -> Result<VoiceEvent> {
	use websocket::ws::receiver::Receiver;
	let message: WsMessage = try!(receiver.recv_message());
	if message.opcode != MessageType::Text {
		return Err(Error::Protocol("Voice websocket message was not Text"))
	}
	let json: serde_json::Value = try!(serde_json::from_reader(&message.payload[..]));
	let original = format!("{:?}", json);
	VoiceEvent::decode(json).map_err(|err| {
		// If there was a decode failure, print the original json for debugging
		warn!("Error vdecoding: {}", original);
		err
	})
}

fn voice_thread(
	endpoint: String,
	mut sender: Sender<WebSocketStream>,
	mut receiver: Receiver<WebSocketStream>,
	channel: mpsc::Receiver<Status>,
) -> Result<()> {
	use sodiumoxide::crypto::secretbox as crypto;
	use opus;

	const SAMPLE_RATE: u32 = 48000;
	const HEADER_LEN: usize = 12;

	// read the first websocket message
	let (interval, port, ssrc, modes) = match try!(recv_message(&mut receiver)) {
		VoiceEvent::Handshake { heartbeat_interval, port, ssrc, modes } => (heartbeat_interval, port, ssrc, modes),
		_ => return Err(Error::Protocol("First voice event was not Handshake"))
	};
	if !modes.iter().find(|&s| s == "xsalsa20_poly1305").is_some() {
		return Err(Error::Protocol("Voice mode \"xsalsa20_poly1305\" unavailable"))
	}

	// bind a UDP socket and send the ssrc value in a packet as identification
	let destination = {
		use std::net::ToSocketAddrs;
		try!(try!((&endpoint[..], port).to_socket_addrs())
			.next()
			.ok_or(Error::Other("Failed to resolve voice hostname")))
	};
	let udp = try!(UdpSocket::bind("0.0.0.0:0"));
	{
		// the length of this packet can be either 4 or 70; if it is 4, voice send works
		// fine, but no own_address is sent back to make voice receive possible
		let mut bytes = [0; 70];
		try!((&mut bytes[..]).write_u32::<BigEndian>(ssrc));
		try!(udp.send_to(&bytes, destination));
	}

	{
		// receive the response to the identification to get port and address info
		let mut bytes = [0; 256];
		let (len, _) = try!(udp.recv_from(&mut bytes));
		let zero_index = bytes.iter().skip(4).position(|&x| x == 0).unwrap();
		let own_address = &bytes[4..4 + zero_index];
		let port_number = try!((&bytes[len - 2..]).read_u16::<LittleEndian>());

		// send the acknowledgement websocket message
		let map = ObjectBuilder::new()
			.insert("op", 1)
			.insert_object("d", |object| object
				.insert("protocol", "udp")
				.insert_object("data", |object| object
					.insert("address", own_address)
					.insert("port", port_number)
					.insert("mode", "xsalsa20_poly1305")
				)
			)
			.unwrap();
		try!(sender.send_message(&WsMessage::text(try!(serde_json::to_string(&map)))));
	}

	// discard websocket messages until we get the Ready
	let encryption_key;
	loop {
		match try!(recv_message(&mut receiver)) {
			VoiceEvent::Ready { mode, secret_key } => {
				encryption_key = crypto::Key::from_slice(&secret_key).expect("failed to create key");
				if mode != "xsalsa20_poly1305" {
					return Err(Error::Protocol("Voice mode in Ready was not \"xsalsa20_poly1305\""))
				}
				break
			}
			VoiceEvent::Unknown(op, value) => debug!("Unknown message type: {}/{:?}", op, value),
			_ => {},
		}
	}

	// start two child threads: one for the voice websocket and another for UDP voice packets
	let receive_chan = {
		let (tx1, rx) = mpsc::channel();
		let tx2 = tx1.clone();
		let udp_clone = try!(udp.try_clone());
		try!(::std::thread::Builder::new()
			.name("Discord Voice Receive WS".into())
			.spawn(move || while let Ok(msg) = recv_message(&mut receiver) {
				match tx1.send(RecvStatus::Websocket(msg)) {
					Ok(()) => {},
					Err(_) => return
				}
			}));
		try!(::std::thread::Builder::new()
			.name("Discord Voice Receive UDP".into())
			.spawn(move || {
				let mut buffer = [0; 512];
				loop {
					let (len, _) = udp_clone.recv_from(&mut buffer).unwrap();
					match tx2.send(RecvStatus::Udp(buffer[..len].iter().cloned().collect())) {
						Ok(()) => {},
						Err(_) => return
					}
				}
			}));
		rx
	};

	// prepare buffers for later use
	let mut sequence = 0;
	let mut timestamp = 0;
	let mut speaking = false;
	let mut audio_source = None;
	let mut receiver = None;

	let mut audio_buffer = [0i16; 960];
	let mut packet = [0u8; 512]; // 256 forces opus to reduce bitrate for some packets
	let mut nonce = crypto::Nonce([0; 24]);
	let mut decoder_map = ::std::collections::HashMap::new();

	let mut opus = try!(opus::Encoder::new(SAMPLE_RATE, opus::Channels::Mono, opus::CodingMode::Audio));
	let mut audio_timer = ::Timer::new(20);
	let mut keepalive_timer = ::Timer::new(interval);
	// after 5 minutes of us sending nothing, Discord will stop sending voice data to us
	let mut audio_keepalive_timer = ::Timer::new(4 * 60 * 1000);

	// start the main loop
	info!("Voice connected to {}", endpoint);
	'outer: loop {
		// Check on the signalling channel
		loop {
			match channel.try_recv() {
				Ok(Status::SetSource(s)) => audio_source = s,
				Ok(Status::SetReceiver(r)) => receiver = r,
				Ok(Status::Poke) => {},
				Err(mpsc::TryRecvError::Empty) => break,
				Err(mpsc::TryRecvError::Disconnected) => break 'outer,
			}
		}

		// Check for received voice data
		if let Some(receiver) = receiver.as_mut() {
			while let Ok(status) = receive_chan.try_recv() {
				match status {
					RecvStatus::Websocket(VoiceEvent::SpeakingUpdate { user_id, ssrc, speaking }) => {
						receiver.speaking_update(ssrc, &user_id, speaking);
					},
					RecvStatus::Websocket(_) => {},
					RecvStatus::Udp(packet) => {
						let mut handle = &packet[2..];
						let sequence = try!(handle.read_u16::<BigEndian>());
						let timestamp = try!(handle.read_u32::<BigEndian>());
						let ssrc = try!(handle.read_u32::<BigEndian>());
						nonce.0[..HEADER_LEN].clone_from_slice(&packet[..HEADER_LEN]);
						if let Ok(decrypted) = crypto::open(&packet[HEADER_LEN..], &nonce, &encryption_key) {
							let channels = try!(opus::packet::get_nb_channels(&decrypted));
							let len = try!(decoder_map.entry((ssrc, channels))
								.or_insert_with(|| opus::Decoder::new(SAMPLE_RATE, channels).unwrap())
								.decode(&decrypted, &mut audio_buffer, false));
							let len = if channels == opus::Channels::Stereo { len * 2 } else { len };
							receiver.voice_packet(ssrc, sequence, timestamp, &audio_buffer[..len]);
						}
					},
				}
			}
		} else {
			// if there's no receiver, discard incoming events
			while let Ok(_) = receive_chan.try_recv() {}
		}

		// Send the voice websocket keepalive if needed
		if keepalive_timer.check_tick() {
			let map = ObjectBuilder::new()
				.insert("op", 3)
				.insert("d", serde_json::Value::Null)
				.unwrap();
			let json = try!(serde_json::to_string(&map));
			try!(sender.send_message(&WsMessage::text(json)));
		}

		// Send the UDP keepalive if needed
		if audio_keepalive_timer.check_tick() {
			let mut bytes = [0; 4];
			try!((&mut bytes[..]).write_u32::<BigEndian>(ssrc));
			try!(udp.send_to(&bytes, destination));
		}

		// read the audio from the source
		let len = match audio_source.as_mut() {
			Some(source) => try!(next_frame(source, &mut audio_buffer)),
			None => 0
		};
		if len == 0 {
			// stop speaking, don't send any audio
			try!(set_speaking(&mut sender, &mut speaking, false));
			audio_timer.sleep_until_tick();
			continue;
		} else if len < audio_buffer.len() {
			// zero-fill the rest of the buffer
			for value in &mut audio_buffer[len..] {
				*value = 0;
			}
		}
		try!(set_speaking(&mut sender, &mut speaking, true));

		// prepare the packet header
		{
			let mut cursor = &mut packet[..HEADER_LEN];
			try!(cursor.write_all(&[0x80, 0x78]));
			try!(cursor.write_u16::<BigEndian>(sequence));
			try!(cursor.write_u32::<BigEndian>(timestamp));
			try!(cursor.write_u32::<BigEndian>(ssrc));
		}
		nonce.0[..HEADER_LEN].clone_from_slice(&packet[..HEADER_LEN]);

		// encode the audio data
		let extent = packet.len() - 16; // leave 16 bytes for encryption overhead
		let len = try!(opus.encode(&audio_buffer, &mut packet[HEADER_LEN..extent]));
		let crypted = crypto::seal(&packet[HEADER_LEN..HEADER_LEN + len], &nonce, &encryption_key);
		packet[HEADER_LEN..HEADER_LEN + crypted.len()].clone_from_slice(&crypted);

		sequence = sequence.wrapping_add(1);
		timestamp = timestamp.wrapping_add(960);

		// wait until the right time, then transmit the packet
		audio_timer.sleep_until_tick();
		try!(udp.send_to(&packet[..HEADER_LEN + crypted.len()], destination));
		audio_keepalive_timer.defer();
	}

	// shutting down the sender like this will also terminate the drain thread
	try!(sender.get_mut().shutdown(::std::net::Shutdown::Both));
	info!("Voice disconnected");
	Ok(())
}

fn next_frame(source: &mut AudioSource, buffer: &mut [i16]) -> Result<usize> {
	for (i, val) in buffer.iter_mut().enumerate() {
		*val = match source.read_i16::<LittleEndian>() {
			Ok(val) => val,
			Err(::byteorder::Error::UnexpectedEOF) => return Ok(i),
			Err(::byteorder::Error::Io(e)) => return Err(From::from(e))
		};
	}
	Ok(buffer.len())
}

fn set_speaking(sender: &mut Sender<WebSocketStream>, store: &mut bool, speaking: bool) -> Result<()> {
	if *store == speaking { return Ok(()) }
	*store = speaking;
	trace!("Speaking: {}", speaking);
	let map = ObjectBuilder::new()
		.insert("op", 5)
		.insert_object("d", |object| object
			.insert("speaking", speaking)
			.insert("delay", 0)
		)
		.unwrap();
	sender.send_message(&WsMessage::text(try!(serde_json::to_string(&map)))).map_err(From::from)
}
