use super::{Result, Error};

use std::io::{Read, Write};
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

/// A websocket connection to the voice servers.
///
/// A VoiceConnection may be active or inactive. Use `voice_connect` and
/// `voice_disconnect` on the `Connection` you are feeding it events from to
/// change what channel it is connected to.
pub struct VoiceConnection {
	user_id: UserId,
	session_id: Option<String>,
	channel: Option<mpsc::Sender<Status>>,
	queue: Vec<AudioSource>,
}

impl VoiceConnection {
	/// Prepare a VoiceConnection for later use.
	pub fn new(user_id: UserId) -> VoiceConnection {
		VoiceConnection {
			user_id: user_id,
			session_id: None,
			channel: None,
			queue: Vec::new(),
		}
	}

	/// Update the voice state based on an event.
	pub fn update(&mut self, event: &Event) {
		match *event {
			Event::VoiceStateUpdate(_, ref voice_state) => {
				if voice_state.user_id == self.user_id {
					self.session_id = Some(voice_state.session_id.clone());
					if !voice_state.channel_id.is_some() {
						self.channel.take(); // drop the connection
					}
				}
			}
			Event::VoiceServerUpdate { ref server_id, ref endpoint, ref token } => {
				self.connect(server_id, endpoint.clone(), token).expect("Voice::connect failure")
			}
			_ => {}
		}
	}

	/// Check whether the voice thread is currently running.
	pub fn is_running(&self) -> bool {
		match self.channel {
			None => false,
			Some(ref channel) => channel.send(Status::NoOp).is_ok(),
		}
	}

	/// Push a source of raw PCM data to the queue.
	#[inline]
	pub fn push_pcm<R: Read + Send + 'static>(&mut self, read: R) {
		self.push_internal(Box::new(read));
	}

	/// Push the path to an audio file which ffmpeg can decode.
	pub fn push_file<P: AsRef<::std::path::Path>>(&mut self, path: P) -> Result<()> {
		use std::process::{Command, Stdio};
		let child = try!(Command::new("ffmpeg")
			.args(&[
				"-i", try!(path.as_ref().to_str().ok_or(Error::Other("File path is not utf8 - fixme?"))),
				"-f", "s16le",
				"-ac", "1",
				"-ar", "48000",
				"-acodec", "pcm_s16le",
				"-"])
			.stdin(Stdio::null())
			.stdout(Stdio::piped())
			.stderr(Stdio::null())
			.spawn());
		let stdout = try!(child.stdout.ok_or(Error::Other("Child process missing stdout")));
		self.push_internal(Box::new(stdout));
		Ok(())
	}

	/// Stop any currently playing audio and clear the queue.
	pub fn stop_audio(&mut self) {
		self.channel.as_ref().map(|ch| ch.send(Status::Clear));
		self.queue.clear();
	}

	fn push_internal(&mut self, source: AudioSource) {
		match self.channel {
			None => self.queue.push(source),
			Some(ref channel) => match channel.send(Status::Push(source)) {
				Ok(()) => {},
				Err(mpsc::SendError(Status::Push(source))) => self.queue.push(source),
				Err(_) => unreachable!()
			}
		}
	}

	fn connect(&mut self, server_id: &ServerId, mut endpoint: String, token: &str) -> Result<()> {
		self.channel.take(); // drop any previous connection

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
		let (tx, rx) = mpsc::channel();
		try!(::std::thread::Builder::new()
			.name("Discord Voice Thread".into())
			.spawn(move || voice_thread(endpoint, sender, receiver, rx).unwrap()));
		for source in ::std::mem::replace(&mut self.queue, Vec::new()) {
			let _ = tx.send(Status::Push(source));
		}
		self.channel = Some(tx);
		Ok(())
	}
}

type AudioSource = Box<::std::io::Read + Send>;

enum Status {
	Push(AudioSource),
	Clear,
	NoOp,
}

fn recv_message(receiver: &mut Receiver<WebSocketStream>) -> Result<VoiceEvent> {
	use websocket::ws::receiver::Receiver;
	let message: WsMessage = try!(receiver.recv_message());
	if message.opcode != MessageType::Text {
		return Err(Error::Other("Got an non-Text frame as voice handshake response"))
	}
	let json: serde_json::Value = try!(serde_json::from_reader(&message.payload[..]));
	let original = format!("{:?}", json);
	VoiceEvent::decode(json).map_err(|err| {
		// If there was a decode failure, print the original json for debugging
		println!("[Warning] Error vdecoding: {}", original);
		err
	})
}

fn voice_thread(
	endpoint: String,
	mut sender: Sender<WebSocketStream>,
	mut receiver: Receiver<WebSocketStream>,
	channel: mpsc::Receiver<Status>,
) -> Result<()> {
	use std::io::Cursor;

	// read the first websocket message
	let (interval, port, ssrc, modes) = match try!(recv_message(&mut receiver)) {
		VoiceEvent::Handshake { heartbeat_interval, port, ssrc, modes } => (heartbeat_interval, port, ssrc, modes),
		_ => return Err(Error::Other("First voice message was not 4/handshake"))
	};
	if !modes.iter().find(|&s| s == "plain").is_some() {
		return Err(Error::Other("Plain voice mode is unavailable"))
	}

	// bind a UDP socket and send the ssrc value in a packet as identification
	let udp = try!(UdpSocket::bind("0.0.0.0:0"));
	let mut bytes = [0; 4];
	try!(Cursor::new(&mut bytes[..]).write_u32::<BigEndian>(ssrc));
	try!(udp.send_to(&bytes, (&endpoint[..], port)));

	// receive the response to the identification to get port and address info
	let mut bytes = [0; 256];
	let (len, _remote_addr) = try!(udp.recv_from(&mut bytes));
	let mut cursor = Cursor::new(&bytes[..len]);
	let _ = try!(cursor.read_u32::<LittleEndian>()); // discard padding
	let port_number = try!(cursor.read_u16::<LittleEndian>());

	// send the acknowledgement websocket message
	let map = ObjectBuilder::new()
		.insert("op", 1)
		.insert_object("d", |object| object
			.insert("protocol", "udp")
			.insert_object("data", |object| object
				.insert("address", "")
				.insert("port", port_number)
				.insert("mode", "plain")
			)
		)
		.unwrap();
	try!(sender.send_message(&WsMessage::text(try!(serde_json::to_string(&map)))));

	// discard websocket messages until we get the Ready
	loop {
		match try!(recv_message(&mut receiver)) {
			VoiceEvent::Ready { mode, secret_key } => {
				if secret_key.len() != 0 {
					println!("[Voice] Got secret key: {:?}", secret_key);
				}
				if mode != "plain" {
					return Err(Error::Other("Voice mode in Ready was not 'plain'"))
				}
				break
			}
			VoiceEvent::Unknown(op, value) => println!("[Voice] Unknown {}/{:?}", op, value),
			_ => {},
		}
	}

	// prepare buffers for later use
	let mut opus = ::utils::OpusEncoder::new().expect("failed new");
	let mut audio_queue = ::std::collections::VecDeque::new();
	let mut audio_buffer = vec![0; 960];
	let mut packet = Vec::with_capacity(256);
	let mut sequence = 0;
	let mut timestamp = 0;

	// tell 'em that we're speaking
	let map = ObjectBuilder::new()
		.insert("op", 5)
		.insert_object("d", |object| object
			.insert("speaking", true)
			.insert("delay", 0)
		)
		.unwrap();
	try!(sender.send_message(&WsMessage::text(try!(serde_json::to_string(&map)))));

	let audio_duration = ::time::Duration::milliseconds(20);
	let keepalive_duration = ::time::Duration::milliseconds(interval as i64);
	let mut audio_timer = ::utils::Timer::new(audio_duration);
	let mut keepalive_timer = ::utils::Timer::new(keepalive_duration);

	// start the main loop
	println!("[Voice] Connected to {}", endpoint);
	'outer: loop {
		::std::thread::sleep_ms(3);

		loop {
			match channel.try_recv() {
				Ok(Status::Push(source)) => {
					audio_queue.push_back(source)
				},
				Ok(Status::Clear) => audio_queue.clear(),
				Ok(Status::NoOp) => {},
				Err(mpsc::TryRecvError::Empty) => break,
				Err(mpsc::TryRecvError::Disconnected) => break 'outer,
			}
		}

		if keepalive_timer.check_and_add(keepalive_duration) {
			let map = ObjectBuilder::new()
				.insert("op", 3)
				.insert("d", serde_json::Value::Null)
				.unwrap();
			let json = try!(serde_json::to_string(&map));
			try!(sender.send_message(&WsMessage::text(json)));
		}

		if audio_timer.check_and_add(audio_duration) && audio_queue.len() > 0 {
			const HEADER_LEN: usize = 12;
			// prepare the packet header
			packet.clear();
			try!(packet.write_all(&[0x80, 0x78]));
			try!(packet.write_u16::<BigEndian>(sequence));
			try!(packet.write_u32::<BigEndian>(timestamp));
			try!(packet.write_u32::<BigEndian>(ssrc));
			let zeroes = packet.capacity() - HEADER_LEN;
			packet.extend(::std::iter::repeat(0).take(zeroes));

			// read the audio from the source
			let len = try!(next_frame(&mut audio_queue[0], &mut audio_buffer[..]));
			if len < audio_buffer.len() {
				// zero-fill the buffer and advance to the next audio source
				for value in &mut audio_buffer[len..] {
					*value = 0;
				}
				audio_queue.pop_front();
			}

			// encode the audio data and transmit it
			let len = opus.encode(&audio_buffer, &mut packet[HEADER_LEN..]).expect("failed encode");
			try!(udp.send_to(&packet[..len + HEADER_LEN], (&endpoint[..], port)));

			sequence = sequence.wrapping_add(1);
			timestamp = timestamp.wrapping_add(960);
		}
	}

	// stop speaking
	::std::thread::sleep_ms(500);
	let map = ObjectBuilder::new()
		.insert("op", 5)
		.insert_object("d", |object| object
			.insert("speaking", false)
			.insert("delay", 0)
		)
		.unwrap();
	try!(sender.send_message(&WsMessage::text(try!(serde_json::to_string(&map)))));

	try!(receiver.get_mut().get_mut().shutdown(::std::net::Shutdown::Both));
	try!(sender.get_mut().shutdown(::std::net::Shutdown::Both));
	Ok(())
}

fn next_frame(source: &mut AudioSource, buffer: &mut [i16]) -> Result<usize> {
	for (i, val) in buffer.iter_mut().enumerate() {
		*val = match source.read_i16::<LittleEndian>() {
			Ok(val) => val / 6, // TODO: add volume controls
			Err(::byteorder::Error::UnexpectedEOF) => return Ok(i),
			Err(::byteorder::Error::Io(e)) => return Err(From::from(e))
		};
	}
	Ok(buffer.len())
}
