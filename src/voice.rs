use super::{Result, Error};

use std::sync::mpsc;
use std::net::UdpSocket;

use websocket::ws::sender::Sender as SenderTrait;
use websocket::client::{Client, Sender, Receiver};
use websocket::stream::WebSocketStream;
use websocket::message::{Message as WsMessage, Type as MessageType};

use serde_json;
use serde_json::builder::ObjectBuilder;

use super::model::*;

/// A websocket connection to the voice servers.
///
/// A VoiceConnection may be active or inactive. Use `voice_connect` and
/// `voice_disconnect` on the `Connection` you are feeding it events from to
/// change what channel it is connected to.
pub struct VoiceConnection {
	user_id: UserId,
	session_id: Option<String>,
	channel: Option<mpsc::Sender<serde_json::Value>>,
}

impl VoiceConnection {
	/// Prepare a VoiceConnection for later use.
	pub fn new(user_id: UserId) -> VoiceConnection {
		VoiceConnection {
			user_id: user_id,
			session_id: None,
			channel: None,
		}
	}

	/// Update the voice state based on an event.
	pub fn update(&mut self, event: &Event) {
		match *event {
			Event::VoiceStateUpdate(_, ref voice_state) => {
				if voice_state.user_id == self.user_id {
					println!("[Debug] Got our session_id");
					self.session_id = Some(voice_state.session_id.clone());
					if !voice_state.channel_id.is_some() {
						self.channel.take(); // drop the connection
					}
				}
			}
			Event::VoiceServerUpdate { ref server_id, ref endpoint, ref token } => {
				println!("[Debug] Connectinating");
				self.connect(server_id, endpoint.clone(), token).expect("Bark Bark")
			}
			_ => {}
		}
	}

	fn connect(&mut self, server_id: &ServerId, mut endpoint: String, token: &str) -> Result<()> {
		println!("Connect()");
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
		println!("Connect(): websocket");
		let response = try!(try!(Client::connect(url)).send());
		try!(response.validate());
		let (mut sender, receiver) = response.begin().split();

		// send the handshake
		println!("Connect(): transmit");
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
		println!("Connect(): start thread");
		let (tx, rx) = mpsc::channel();
		try!(::std::thread::Builder::new()
			.name("Discord Voice Thread".into())
			.spawn(move || voice_thread(endpoint, sender, receiver, rx).unwrap()));
		self.channel = Some(tx);
		Ok(())
	}

	/* /// Cleanly shut down the websocket connection. Optional.
	pub fn shutdown(mut self) -> Result<()> {
		let _ = self.keepalive_channel.send(Status::Shutdown);
		try!(self.receiver.get_mut().get_mut().shutdown(::std::net::Shutdown::Both));
		Ok(())
	} */
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
	channel: mpsc::Receiver<serde_json::Value>,
) -> Result<()> {
	use std::io::{Cursor, Read, Write};
	use byteorder::{LittleEndian, BigEndian, WriteBytesExt, ReadBytesExt};

	// bind a UDP socket
	let udp = try!(UdpSocket::bind("0.0.0.0:0"));
	let udp_port = try!(udp.local_addr()).port();
	println!("UDP port: {}", udp_port);

	// read the first message...
	let (interval, port, ssrc, modes) = match try!(recv_message(&mut receiver)) {
		VoiceEvent::Handshake { heartbeat_interval, port, ssrc, modes } => (heartbeat_interval, port, ssrc, modes),
		_ => return Err(Error::Other("First voice message was not 4/handshake"))
	};
	if !modes.iter().find(|&s| s == "plain").is_some() {
		return Err(Error::Other("Plain voice mode is unavailable"))
	}

	// pack the ssrc value into a packet and send it
	let mut bytes = [0; 4];
	try!(Cursor::new(&mut bytes[..]).write_u32::<BigEndian>(ssrc));
	try!(udp.send_to(&bytes, (&endpoint[..], port)));

	// receive the packet from the Discord servers
	let mut bytes = [0; 256];
	let (len, _remote_addr) = try!(udp.recv_from(&mut bytes));
	println!("Got UDP packet!! {:?}", &bytes[..len]);
	let mut cursor = Cursor::new(&bytes[..len]);
	let _ = try!(cursor.read_u32::<LittleEndian>());
	let remote_port = try!(cursor.read_u16::<LittleEndian>());
	println!("Port number: {}", remote_port);

	// send the acknowledgement message
	let map = ObjectBuilder::new()
		.insert("op", 1)
		.insert_object("d", |object| object
			.insert("protocol", "udp")
			.insert_object("data", |object| object
				.insert("address", "")
				.insert("port", remote_port)
				.insert("mode", "plain")
			)
		)
		.unwrap();
	try!(sender.send_message(&WsMessage::text(try!(serde_json::to_string(&map)))));

	// discard messages until we get the Ready
	loop {
		match try!(recv_message(&mut receiver)) {
			VoiceEvent::Ready { mode } => {
				if mode != "plain" {
					return Err(Error::Other("Voice mode in Ready was not 'plain'"))
				}
				break
			}
			VoiceEvent::Unknown(op, value) => println!("[Voice] Unknown {}/{:?}", op, value),
			_ => {},
		}
	}
	println!("[Voice] Ready");
	::std::thread::sleep_ms(500);

	// tell 'em that we're speaking
	let map = ObjectBuilder::new()
		.insert("op", 5)
		.insert_object("d", |object| object
			.insert("speaking", true)
			.insert("delay", 0)
		)
		.unwrap();
	try!(sender.send_message(&WsMessage::text(try!(serde_json::to_string(&map)))));

	let mut opus = ::utils::OpusEncoder::new().expect("failed new");
	let mut file = try!(::std::fs::File::open("../res/discord-swing.pcm"));
	let mut test_data = Vec::with_capacity(960);
	let mut packet = Vec::with_capacity(2048);
	let mut sequence = 0;
	let mut timestamp = 0;

	// prepare the encoded packet
	for i in 0.. {
		print!("Encoding:{}", i);

		packet.clear();
		try!(packet.write_all(&[0x80, 0x78]));
		try!(packet.write_u16::<BigEndian>(sequence));
		try!(packet.write_u32::<BigEndian>(timestamp));
		try!(packet.write_u32::<BigEndian>(ssrc));
		packet.extend(::std::iter::repeat(0).take(2048-12));

		// read the test data
		test_data.clear();
		for _ in 0..test_data.capacity() {
			test_data.push(try!(file.read_i16::<LittleEndian>()) / 4);
		}

		// encode the test data
		print!(" Seq:{}:{}", sequence, timestamp);
		print!(" Input:{}:{}", test_data.len(), ::std::mem::size_of_val(&test_data[..]));
		let len = opus.encode(&test_data, &mut packet[12..]).expect("failed encode");

		// transmit the encoded packet
		try!(udp.send_to(&packet[..len + 12], (&endpoint[..], port)));

		println!(" Sent:{}", len + 12);
		::std::thread::sleep_ms(17);
		sequence = sequence.wrapping_add(1);
		timestamp = timestamp.wrapping_add(960);
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

	/*loop {
		let mut bytes = [0; 1024];
		let (len, remote_addr) = try!(udp.recv_from(&mut bytes));
		println!("Got UDP packet!! {:?}", &bytes[..len]);
	}*/

	// start the main loop
	let mut countdown = interval;
	'outer: loop {
		// TODO: this is not a precise timer, but it's good enough for now
		::std::thread::sleep_ms(100);
		countdown = countdown.saturating_sub(100);

		loop {
			match channel.try_recv() {
				Ok(val) => {
					let json = try!(serde_json::to_string(&val));
					try!(sender.send_message(&WsMessage::text(json)));
				},
				Err(mpsc::TryRecvError::Empty) => break,
				Err(mpsc::TryRecvError::Disconnected) => break 'outer,
			}
		}

		if countdown == 0 {
			countdown = interval;
			let map = ObjectBuilder::new()
				.insert("op", 3)
				.insert("d", serde_json::Value::Null)
				.unwrap();
			let json = try!(serde_json::to_string(&map));
			try!(sender.send_message(&WsMessage::text(json)));
		}
	}
	let _ = sender.get_mut().shutdown(::std::net::Shutdown::Both);
	Ok(())
}

//fn recv_thread(mut receiver: Receiver<WebSocketStream>) { // , channel: mpsc::Receiver<serde_json::Value>) {
//}
