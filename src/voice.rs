//! Voice communication module.
//!
//! A `VoiceConnection` for a server is obtained from a `Connection`. It can then be used to
//! join a channel, change mute/deaf status, and play and receive audio.

use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::sync::mpsc;
use std::net::UdpSocket;

use byteorder::{LittleEndian, BigEndian, WriteBytesExt, ReadBytesExt};
use opus;
use serde_json;
use serde_json::builder::ObjectBuilder;
use sodiumoxide::crypto::secretbox as crypto;
use websocket::client::{Client, Sender};
use websocket::stream::WebSocketStream;

use model::*;
use {Result, Error, SenderExt, ReceiverExt};

/// An active or inactive voice connection, obtained from `Connection::voice`.
pub struct VoiceConnection {
	// primary WS send control
	server_id: Option<ServerId>, // None for group and private calls
	user_id: UserId,
	main_ws: mpsc::Sender<::internal::Status>,
	channel_id: Option<ChannelId>,
	mute: bool,
	deaf: bool,

	// main WS receive control
	session_id: Option<String>,
	endpoint_token: Option<(String, String)>,

	// voice thread (voice WS + UDP) control
	sender: mpsc::Sender<Status>,
}

/// A readable audio source.
pub trait AudioSource: Send {
	/// Called each frame to determine if the audio source is stereo.
	///
	/// This value should change infrequently; changing it will reset the encoder state.
	fn is_stereo(&mut self) -> bool;

	/// Called each frame when more audio is required.
	///
	/// Samples should be supplied at 48000Hz, and if `is_stereo` returned true, the channels
	/// should be interleaved, left first.
	///
	/// The result should normally be `Some(N)`, where `N` is the number of samples written to the
	/// buffer. The rest of the buffer is zero-filled; the whole buffer must be filled each call
	/// to avoid audio interruptions.
	///
	/// If `Some(0)` is returned, no audio will be sent this frame, but the audio source will
	/// remain active. If `None` is returned, the audio source is considered to have ended, and
	/// `read_frame` will not be called again.
	fn read_frame(&mut self, buffer: &mut [i16]) -> Option<usize>;
}

/// A receiver for incoming audio.
pub trait AudioReceiver: Send {
	/// Called when a user's currently-speaking state has updated.
	///
	/// This method is the only way to know the `ssrc` to `user_id` mapping, but is unreliable and
	/// only a hint for when users are actually speaking, due both to latency differences and that
	/// it is possible for a user to leave `speaking` true even when they are not sending audio.
	fn speaking_update(&mut self, ssrc: u32, user_id: UserId, speaking: bool);

	/// Called when a voice packet is received.
	///
	/// The sequence number increases by one per packet sent, and can be used to reorder packets
	/// if they have been received out of order. The timestamp increases at 48000Hz (typically by
	/// 960 per 20ms frame). If `stereo` is true, the length of the `data` slice is doubled and
	/// samples have been interleaved. The typical length of `data` is 960 or 1920 for a 20ms frame,
	/// but may be larger or smaller in some situations.
	fn voice_packet(&mut self, ssrc: u32, sequence: u16, timestamp: u32, stereo: bool, data: &[i16]);
}

impl VoiceConnection {
	#[doc(hidden)]
	pub fn __new(server_id: Option<ServerId>, user_id: UserId, main_ws: mpsc::Sender<::internal::Status>) -> Self {
		let (tx, rx) = mpsc::channel();
		start_voice_thread(server_id, rx);
		VoiceConnection {
			server_id: server_id,
			user_id: user_id,
			main_ws: main_ws,
			channel_id: None,
			mute: false,
			deaf: false,
			session_id: None,
			endpoint_token: None,
			sender: tx,
		}
	}

	/// Connect to the specified voice channel. Any previous channel on this server will be
	/// disconnected from.
	#[inline]
	pub fn connect(&mut self, channel_id: ChannelId) {
		self.channel_id = Some(channel_id);
		self.send_connect();
	}

	/// Disconnect from the current voice channel, if any.
	#[inline]
	pub fn disconnect(&mut self) {
		self.channel_id = None;
		self.send_connect();
	}

	/// Set the mute status of the voice connection.
	///
	/// Note that enabling mute client-side is cosmetic and does not prevent the sending of audio;
	/// to fully mute, you must manually silence the audio source.
	#[inline]
	pub fn set_mute(&mut self, mute: bool) {
		self.mute = mute;
		if self.channel_id.is_some() { self.send_connect() }
	}

	/// Set the deaf status of the voice connection. Does not affect mute status.
	#[inline]
	pub fn set_deaf(&mut self, deaf: bool) {
		self.deaf = deaf;
		if self.channel_id.is_some() { self.send_connect() }
	}

	/// Get the current channel of this voice connection, if any.
	#[inline]
	pub fn current_channel(&self) -> Option<ChannelId> {
		self.channel_id
	}

	/// Send the connect/disconnect command over the main websocket
	fn send_connect(&self) {
		let _ = self.main_ws.send(::internal::Status::SendMessage(ObjectBuilder::new()
			.insert("op", 4)
			.insert_object("d", |object| object
				.insert("guild_id", self.server_id.map(|s| s.0))
				.insert("channel_id", self.channel_id.map(|c| c.0))
				.insert("self_mute", self.mute)
				.insert("self_deaf", self.deaf)
			)
			.build()
		));
	}

	#[doc(hidden)]
	pub fn __update_state(&mut self, voice_state: &VoiceState) {
		if voice_state.user_id == self.user_id {
			self.channel_id = voice_state.channel_id;
			if voice_state.channel_id.is_some() {
				let session_id = voice_state.session_id.clone();
				if let Some((endpoint, token)) = self.endpoint_token.take() {
					self.internal_connect(session_id, endpoint, token);
				} else {
					self.session_id = Some(session_id);
				}
			} else {
				self.internal_disconnect();
			}
		}
	}

	#[doc(hidden)]
	pub fn __update_server(&mut self, endpoint: &Option<String>, token: &String) {
		if let Some(endpoint) = endpoint.clone() {
			let token = token.clone();
			// nb: .take() is not used; in the event of server transfer, only this is called
			if let Some(session_id) = self.session_id.clone() {
				self.internal_connect(session_id, endpoint, token);
			} else {
				self.endpoint_token = Some((endpoint, token));
			}
		} else {
			self.internal_disconnect();
		}
	}

	/// Play from the given audio source.
	#[inline]
	pub fn play(&mut self, source: Box<AudioSource>) {
		self.thread_send(Status::SetSource(Some(source)));
	}

	/// Stop the currently playing audio source.
	#[inline]
	pub fn stop(&mut self) {
		self.thread_send(Status::SetSource(None));
	}

	/// Set the receiver to which incoming voice will be sent.
	#[inline]
	pub fn set_receiver(&mut self, receiver: Box<AudioReceiver>) {
		self.thread_send(Status::SetReceiver(Some(receiver)));
	}

	/// Clear the voice receiver, discarding incoming voice.
	#[inline]
	pub fn clear_receiver(&mut self) {
		self.thread_send(Status::SetReceiver(None));
	}

	fn thread_send(&mut self, status: Status) {
		match self.sender.send(status) {
			Ok(()) => {}
			Err(mpsc::SendError(status)) => {
				// voice thread has crashed... start it over again
				let (tx, rx) = mpsc::channel();
				self.sender = tx;
				self.sender.send(status).unwrap(); // should be infallible
				debug!("Restarting crashed voice thread...");
				start_voice_thread(self.server_id, rx);
				self.send_connect();
			}
		}
	}

	#[inline]
	fn internal_disconnect(&mut self) {
		self.thread_send(Status::Disconnect);
	}

	#[inline]
	fn internal_connect(&mut self, session_id: String, endpoint: String, token: String) {
		let user_id = self.user_id;
		let server_id = match (&self.server_id, &self.channel_id) {
			(&Some(ServerId(id)), _) | (&None, &Some(ChannelId(id))) => id,
			_ => {
				error!("no server_id or channel_id in internal_connect");
				return;
			}
		};
		self.thread_send(Status::Connect(ConnStartInfo {
			server_id: server_id,
			user_id: user_id,
			session_id: session_id,
			endpoint: endpoint,
			token: token,
		}));
	}
}

impl Drop for VoiceConnection {
	fn drop(&mut self) {
		self.disconnect();
	}
}

/// Create an audio source based on a `pcm_s16le` input stream.
///
/// The input data should be in signed 16-bit little-endian PCM input stream at 48000Hz. If
/// `stereo` is true, the channels should be interleaved, left first.
pub fn create_pcm_source<R: Read + Send + 'static>(stereo: bool, read: R) -> Box<AudioSource> {
	Box::new(PcmSource(stereo, read))
}

struct PcmSource<R: Read + Send>(bool, R);

impl<R: Read + Send> AudioSource for PcmSource<R> {
	fn is_stereo(&mut self) -> bool { self.0 }
	fn read_frame(&mut self, buffer: &mut [i16]) -> Option<usize> {
		for (i, val) in buffer.iter_mut().enumerate() {
			*val = match self.1.read_i16::<LittleEndian>() {
				Ok(val) => val,
				Err(ref e) if e.kind() == io::ErrorKind::UnexpectedEof => return Some(i),
				Err(_) => return None
			}
		}
		Some(buffer.len())
	}
}

/// Use `ffmpeg` to open an audio file as a PCM stream.
///
/// Requires `ffmpeg` to be on the path and executable. If `ffprobe` is available and indicates
/// that the input file is stereo, the returned audio source will be stereo.
pub fn open_ffmpeg_stream<P: AsRef<::std::ffi::OsStr>>(path: P) -> Result<Box<AudioSource>> {
	use std::process::{Command, Stdio};
	let path = path.as_ref();
	let stereo = check_stereo(path).unwrap_or(false);
	let child = try!(Command::new("ffmpeg")
		.arg("-i").arg(path)
		.args(&[
			"-f", "s16le",
			"-ac", if stereo { "2" } else { "1" },
			"-ar", "48000",
			"-acodec", "pcm_s16le",
			"-"])
		.stdin(Stdio::null())
		.stdout(Stdio::piped())
		.stderr(Stdio::null())
		.spawn());
	Ok(create_pcm_source(stereo, ProcessStream(child)))
}

fn check_stereo(path: &::std::ffi::OsStr) -> Result<bool> {
	use std::process::{Command, Stdio};
	let output = try!(Command::new("ffprobe")
		.args(&["-v", "quiet", "-of", "json", "-show_streams", "-i"])
		.arg(path)
		.stdin(Stdio::null())
		.output());
	let json: serde_json::Value = try!(serde_json::from_reader(&output.stdout[..]));
	let streams = try!(json.as_object()
		.and_then(|m| m.get("streams"))
		.and_then(|v| v.as_array())
		.ok_or(Error::Other("")));
	Ok(streams.iter().any(|stream|
		stream.as_object().and_then(|m| m.get("channels").and_then(|v| v.as_i64())) == Some(2)
	))
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

/// Use `youtube-dl` and `ffmpeg` to stream from an internet source.
///
/// Requires both `youtube-dl` and `ffmpeg` to be on the path and executable.
/// On Windows, this means the `.exe` version of `youtube-dl` must be used.
///
/// The audio download is streamed rather than downloaded in full; this may be desireable for
/// longer audios but can introduce occasional brief interruptions.
pub fn open_ytdl_stream(url: &str) -> Result<Box<AudioSource>> {
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
		return Err(Error::Command("youtube-dl", output));
	}

	let json: serde_json::Value = try!(serde_json::from_reader(&output.stdout[..]));
	let map = match json.as_object() {
		Some(map) => map,
		None => return Err(Error::Other("youtube-dl output could not be read"))
	};
	let url = match map.get("url").and_then(serde_json::Value::as_str) {
		Some(url) => url,
		None => return Err(Error::Other("youtube-dl output's \"url\" could not be read"))
	};
	open_ffmpeg_stream(url)
}

enum Status {
	SetSource(Option<Box<AudioSource>>),
	SetReceiver(Option<Box<AudioReceiver>>),
	Connect(ConnStartInfo),
	Disconnect,
}

fn start_voice_thread(server_id: Option<ServerId>, rx: mpsc::Receiver<Status>) {
	let name = match server_id {
		Some(ServerId(id)) => format!("discord voice (server {})", id),
		None => "discord voice (private/groups)".to_owned(),
	};
	::std::thread::Builder::new()
		.name(name)
		.spawn(move || voice_thread(rx))
		.expect("Failed to start voice thread");
}

fn voice_thread(channel: mpsc::Receiver<Status>) {
	let mut audio_source = None;
	let mut receiver = None;
	let mut connection = None;
	let mut audio_timer = ::Timer::new(20);

	// start the main loop
	'outer: loop {
		// Check on the signalling channel
		loop {
			match channel.try_recv() {
				Ok(Status::SetSource(s)) => audio_source = s,
				Ok(Status::SetReceiver(r)) => receiver = r,
				Ok(Status::Connect(info)) => {
					connection = InternalConnection::new(info).map_err(
						|e| error!("Error connecting to voice: {:?}", e)
					).ok();
				},
				Ok(Status::Disconnect) => connection = None,
				Err(mpsc::TryRecvError::Empty) => break,
				Err(mpsc::TryRecvError::Disconnected) => break 'outer,
			}
		}

		// Update the voice connection, transmitting and receiving data as needed
		let mut error = false;
		if let Some(connection) = connection.as_mut() {
			// update() will sleep using audio_timer as needed
			if let Err(e) = connection.update(&mut audio_source, &mut receiver, &mut audio_timer) {
				error!("Error in voice connection: {:?}", e);
				error = true;
			}
		} else {
			// no connection, so we sleep ourselves
			audio_timer.sleep_until_tick();
		}
		if error {
			connection = None;
		}
	}
}

struct ConnStartInfo {
	// may have originally been a ServerId or ChannelId
	server_id: u64,
	user_id: UserId,
	endpoint: String,
	session_id: String,
	token: String,
}

struct InternalConnection {
	sender: Sender<WebSocketStream>,
	receive_chan: mpsc::Receiver<RecvStatus>,
	encryption_key: crypto::Key,
	udp: UdpSocket,
	destination: ::std::net::SocketAddr,
	ssrc: u32,
	sequence: u16,
	timestamp: u32,
	speaking: bool,
	silence_frames: u8,
	decoder_map: HashMap<(u32, opus::Channels), opus::Decoder>,
	encoder: opus::Encoder,
	encoder_stereo: bool,
	keepalive_timer: ::Timer,
	audio_keepalive_timer: ::Timer,
}

const SAMPLE_RATE: u32 = 48000;
const HEADER_LEN: usize = 12;

impl InternalConnection {
	fn new(info: ConnStartInfo) -> Result<InternalConnection> {
		let ConnStartInfo { server_id, user_id, mut endpoint, session_id, token } = info;

		// prepare the URL: drop the :80 and prepend wss://
		if endpoint.ends_with(":80") {
			let len = endpoint.len();
			endpoint.truncate(len - 3);
		}
		// establish the websocket connection
		let url = match ::websocket::client::request::Url::parse(&format!("wss://{}", endpoint)) {
			Ok(url) => url,
			Err(_) => return Err(Error::Other("Invalid endpoint URL"))
		};
		let response = try!(try!(Client::connect(url)).send());
		try!(response.validate());
		let (mut sender, mut receiver) = response.begin().split();

		// send the handshake
		let map = ObjectBuilder::new()
			.insert("op", 0)
			.insert_object("d", |object| object
				.insert("server_id", server_id)
				.insert("user_id", user_id.0)
				.insert("session_id", session_id)
				.insert("token", token)
			)
			.build();
		try!(sender.send_json(&map));

		let stuff;
		loop {
			match try!(receiver.recv_json(VoiceEvent::decode)) {
				VoiceEvent::Heartbeat { .. } => {
					// TODO: handle this by beginning to heartbeat at the
					// supplied interval
				}
				VoiceEvent::Handshake { heartbeat_interval, port, ssrc, modes, ip } => {
					stuff = (heartbeat_interval, port, ssrc, modes, ip);
					break;
				}
				other => {
					debug!("Unexpected voice msg: {:?}", other);
					return Err(Error::Protocol("Unexpected message setting up voice"));
				}
			}
		}
		let (interval, port, ssrc, modes, ip) = stuff;
		if !modes.iter().any(|s| s == "xsalsa20_poly1305") {
			return Err(Error::Protocol("Voice mode \"xsalsa20_poly1305\" unavailable"))
		}

		// bind a UDP socket and send the ssrc value in a packet as identification
		let destination = {
			use std::net::ToSocketAddrs;
			try!(try!((ip.as_ref().map(|ip| &ip[..]).unwrap_or(&endpoint[..]), port).to_socket_addrs())
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
				.build();
			try!(sender.send_json(&map));
		}

		// discard websocket messages until we get the Ready
		let encryption_key;
		loop {
			match try!(receiver.recv_json(VoiceEvent::decode)) {
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
		let thread = ::std::thread::current();
		let thread_name = thread.name().unwrap_or("discord voice");
		let receive_chan = {
			let (tx1, rx) = mpsc::channel();
			let tx2 = tx1.clone();
			let udp_clone = try!(udp.try_clone());
			try!(::std::thread::Builder::new()
				.name(format!("{} (WS reader)", thread_name))
				.spawn(move || while let Ok(msg) = receiver.recv_json(VoiceEvent::decode) {
					match tx1.send(RecvStatus::Websocket(msg)) {
						Ok(()) => {},
						Err(_) => return
					}
				}));
			try!(::std::thread::Builder::new()
				.name(format!("{} (UDP reader)", thread_name))
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

		info!("Voice connected to {} ({})", endpoint, destination);
		Ok(InternalConnection {
			sender: sender,
			receive_chan: receive_chan,
			encryption_key: encryption_key,
			udp: udp,
			destination: destination,

			ssrc: ssrc,
			sequence: 0,
			timestamp: 0,
			speaking: false,
			silence_frames: 0,

			decoder_map: HashMap::new(),
			encoder: try!(opus::Encoder::new(SAMPLE_RATE, opus::Channels::Mono, opus::CodingMode::Audio)),
			encoder_stereo: false,
			keepalive_timer: ::Timer::new(interval),
			// after 5 minutes of us sending nothing, Discord will stop sending voice data to us
			audio_keepalive_timer: ::Timer::new(4 * 60 * 1000),
		})
	}

	fn update(&mut self,
		source: &mut Option<Box<AudioSource>>,
		receiver: &mut Option<Box<AudioReceiver>>,
		audio_timer: &mut ::Timer,
	) -> Result<()> {
		let mut audio_buffer = [0i16; 960 * 2]; // 20 ms, stereo
		let mut packet = [0u8; 512]; // 256 forces opus to reduce bitrate for some packets
		let mut nonce = crypto::Nonce([0; 24]);

		// Check for received voice data
		if let Some(receiver) = receiver.as_mut() {
			while let Ok(status) = self.receive_chan.try_recv() {
				match status {
					RecvStatus::Websocket(VoiceEvent::SpeakingUpdate { user_id, ssrc, speaking }) => {
						receiver.speaking_update(ssrc, user_id, speaking);
					},
					RecvStatus::Websocket(_) => {},
					RecvStatus::Udp(packet) => {
						let mut handle = &packet[2..];
						let sequence = try!(handle.read_u16::<BigEndian>());
						let timestamp = try!(handle.read_u32::<BigEndian>());
						let ssrc = try!(handle.read_u32::<BigEndian>());
						nonce.0[..HEADER_LEN].clone_from_slice(&packet[..HEADER_LEN]);
						if let Ok(decrypted) = crypto::open(&packet[HEADER_LEN..], &nonce, &self.encryption_key) {
							let channels = try!(opus::packet::get_nb_channels(&decrypted));
							let len = try!(self.decoder_map.entry((ssrc, channels))
								.or_insert_with(|| opus::Decoder::new(SAMPLE_RATE, channels).unwrap())
								.decode(&decrypted, &mut audio_buffer, false));
							let stereo = channels == opus::Channels::Stereo;
							receiver.voice_packet(ssrc, sequence, timestamp,
								stereo, &audio_buffer[..if stereo { len * 2 } else { len }]);
						}
					},
				}
			}
		} else {
			// if there's no receiver, discard incoming events
			while let Ok(_) = self.receive_chan.try_recv() {}
		}

		// Send the voice websocket keepalive if needed
		if self.keepalive_timer.check_tick() {
			let map = ObjectBuilder::new()
				.insert("op", 3)
				.insert("d", serde_json::Value::Null)
				.build();
			try!(self.sender.send_json(&map));
		}

		// Send the UDP keepalive if needed
		if self.audio_keepalive_timer.check_tick() {
			let mut bytes = [0; 4];
			try!((&mut bytes[..]).write_u32::<BigEndian>(self.ssrc));
			try!(self.udp.send_to(&bytes, self.destination));
		}

		// read the audio from the source
		let mut clear_source = false;
		let len = if let Some(source) = source.as_mut() {
			let stereo = source.is_stereo();
			if stereo != self.encoder_stereo {
				let channels = if stereo { opus::Channels::Stereo } else { opus::Channels::Mono };
				self.encoder = try!(opus::Encoder::new(SAMPLE_RATE, channels, opus::CodingMode::Audio));
				self.encoder_stereo = stereo;
			}
			let buffer_len = if stereo { 960 * 2 } else { 960 };
			match source.read_frame(&mut audio_buffer[..buffer_len]) {
				Some(len) => len,
				None => { clear_source = true; 0 }
			}
		} else {
			0
		};
		if clear_source {
			*source = None;
		}
		if len == 0 {
			// stop speaking, don't send any audio
			try!(self.set_speaking(false));
			if self.silence_frames > 0 {
				// send a few frames of silence; could be optimized to be pre-encoded
				self.silence_frames -= 1;
				for value in &mut audio_buffer[..] {
					*value = 0;
				}
			} else {
				audio_timer.sleep_until_tick();
				return Ok(());
			}
		} else {
			self.silence_frames = 5;
			// zero-fill the rest of the buffer
			for value in &mut audio_buffer[len..] {
				*value = 0;
			}
		}
		try!(self.set_speaking(true));

		// prepare the packet header
		{
			let mut cursor = &mut packet[..HEADER_LEN];
			try!(cursor.write_all(&[0x80, 0x78]));
			try!(cursor.write_u16::<BigEndian>(self.sequence));
			try!(cursor.write_u32::<BigEndian>(self.timestamp));
			try!(cursor.write_u32::<BigEndian>(self.ssrc));
			debug_assert!(cursor.len() == 0);
		}
		nonce.0[..HEADER_LEN].clone_from_slice(&packet[..HEADER_LEN]);

		// encode the audio data
		let extent = packet.len() - 16; // leave 16 bytes for encryption overhead
		let buffer_len = if self.encoder_stereo { 960 * 2 } else { 960 };
		let len = try!(self.encoder.encode(&audio_buffer[..buffer_len], &mut packet[HEADER_LEN..extent]));
		let crypted = crypto::seal(&packet[HEADER_LEN..HEADER_LEN + len], &nonce, &self.encryption_key);
		packet[HEADER_LEN..HEADER_LEN + crypted.len()].clone_from_slice(&crypted);

		self.sequence = self.sequence.wrapping_add(1);
		self.timestamp = self.timestamp.wrapping_add(960);

		// wait until the right time, then transmit the packet
		audio_timer.sleep_until_tick();
		try!(self.udp.send_to(&packet[..HEADER_LEN + crypted.len()], self.destination));
		self.audio_keepalive_timer.defer();
		Ok(())
	}

	fn set_speaking(&mut self, speaking: bool) -> Result<()> {
		if self.speaking == speaking {
			return Ok(())
		}
		self.speaking = speaking;
		let map = ObjectBuilder::new()
			.insert("op", 5)
			.insert_object("d", |object| object
				.insert("speaking", speaking)
				.insert("delay", 0)
			)
			.build();
		self.sender.send_json(&map)
	}
}

impl Drop for InternalConnection {
	fn drop(&mut self) {
		// shutting down the sender like this should also terminate the read threads
		let _ = self.sender.get_mut().shutdown(::std::net::Shutdown::Both);
		info!("Voice disconnected");
	}
}

enum RecvStatus {
	Websocket(VoiceEvent),
	Udp(Vec<u8>),
}
