use std::sync::mpsc;
#[cfg(feature="voice")]
use std::collections::HashMap;

use websocket::client::{Client, Sender, Receiver};
use websocket::stream::WebSocketStream;

use serde_json;
use serde_json::builder::ObjectBuilder;

use model::*;
use internal::Status;
#[cfg(feature="voice")]
use voice::VoiceConnection;
use {Result, Error, SenderExt, ReceiverExt};

const GATEWAY_VERSION: u64 = 5;

#[cfg(feature="voice")]
macro_rules! finish_connection {
	($($name1:ident: $val1:expr),*; $($name2:ident: $val2:expr,)*) => { Connection {
		$($name1: $val1,)*
		$($name2: $val2,)*
	}}
}
#[cfg(not(feature="voice"))]
macro_rules! finish_connection {
	($($name1:ident: $val1:expr),*; $($name2:ident: $val2:expr,)*) => { Connection {
		$($name1: $val1,)*
	}}
}

#[cfg(feature="voice")]
macro_rules! voice_only {
	($b:block) => {$b}
}
#[cfg(not(feature="voice"))]
macro_rules! voice_only {
	($b:block) => {}
}

/// Websocket connection to the Discord servers.
pub struct Connection {
	keepalive_channel: mpsc::Sender<Status>,
	receiver: Receiver<WebSocketStream>,
	#[cfg(feature="voice")]
	voice_handles: HashMap<ServerId, VoiceConnection>,
	#[cfg(feature="voice")]
	user_id: UserId,
	ws_url: String,
	token: String,
	session_id: Option<String>,
	last_sequence: u64,
}

impl Connection {
	/// Establish a connection to the Discord websocket servers.
	///
	/// Returns both the `Connection` and the `ReadyEvent` which is always the
	/// first event received and contains initial state information.
	///
	/// Usually called internally by `Discord::connect`, which provides both
	/// the token and URL.
	pub fn new(base_url: &str, token: &str) -> Result<(Connection, ReadyEvent)> {
		debug!("Gateway: {}", base_url);
		// establish the websocket connection
		let url = try!(build_gateway_url(base_url));
		let response = try!(try!(Client::connect(url)).send());
		try!(response.validate());
		let (mut sender, mut receiver) = response.begin().split();

		// send the handshake
		let identify = identify(token);
		try!(sender.send_json(&identify));

		// read the Hello and spawn the keepalive thread
		let heartbeat_interval;
		match try!(receiver.recv_json(GatewayEvent::decode)) {
			GatewayEvent::Hello(interval) => heartbeat_interval = interval,
			other => {
				debug!("Unexpected event: {:?}", other);
				return Err(Error::Protocol("Expected Hello during handshake"))
			}
		}

		let (tx, rx) = mpsc::channel();
		try!(::std::thread::Builder::new()
			.name("Discord Keepalive".into())
			.spawn(move || keepalive(heartbeat_interval, sender, rx)));

		// read the Ready event
		let sequence;
		let ready;
		match try!(receiver.recv_json(GatewayEvent::decode)) {
			GatewayEvent::Dispatch(seq, Event::Ready(event)) => {
				sequence = seq;
				ready = event;
			},
			GatewayEvent::InvalidateSession => {
				debug!("Session invalidated, reidentifying");
				let _ = tx.send(Status::SendMessage(identify));
				match try!(receiver.recv_json(GatewayEvent::decode)) {
					GatewayEvent::Dispatch(seq, Event::Ready(event)) => {
						sequence = seq;
						ready = event;
					}
					other => {
						debug!("Unexpected event: {:?}", other);
						return Err(Error::Protocol("Expected Ready during handshake"))
					}
				}
			}
			other => {
				debug!("Unexpected event: {:?}", other);
				return Err(Error::Protocol("Expected Ready or InvalidateSession during handshake"))
			}
		}
		if ready.version != GATEWAY_VERSION {
			warn!("Got protocol version {} instead of {}", ready.version, GATEWAY_VERSION);
		}
		let session_id = ready.session_id.clone();

		// return the connection
		Ok((finish_connection!(
			keepalive_channel: tx,
			receiver: receiver,
			ws_url: base_url.to_owned(),
			token: token.to_owned(),
			session_id: Some(session_id),
			last_sequence: sequence;
			// voice only
			user_id: ready.user.id,
			voice_handles: HashMap::new(),
		), ready))
	}

	/// Change the game information that this client reports as playing.
	pub fn set_game(&self, game: Option<Game>) {
		let msg = ObjectBuilder::new()
			.insert("op", 3)
			.insert_object("d", move |mut object| {
				object = object.insert("idle_since", serde_json::Value::Null);
				match game {
					Some(game) => object.insert_object("game", move |o| o.insert("name", game.name)),
					None => object.insert("game", serde_json::Value::Null),
				}
			})
			.unwrap();
		let _ = self.keepalive_channel.send(Status::SendMessage(msg));
	}

	/// Set the client to be playing this game, with defaults used for any
	/// extended information.
	pub fn set_game_name(&self, name: String) {
		self.set_game(Some(Game::playing(name)));
	}

	/// Get a handle to the voice connection for a server.
	#[cfg(feature="voice")]
	pub fn voice(&mut self, server_id: ServerId) -> &mut VoiceConnection {
		let Connection { ref mut voice_handles, user_id, ref keepalive_channel, .. } = *self;
		voice_handles.entry(server_id).or_insert_with(||
			VoiceConnection::__new(server_id, user_id, keepalive_channel.clone())
		)
	}

	/// Drop the voice connection for a server, forgetting all settings.
	///
	/// Calling `.voice(server_id).disconnect()` will disconnect from voice but retain the mute
	/// and deaf status, audio source, and audio receiver.
	#[cfg(feature="voice")]
	pub fn drop_voice(&mut self, server_id: ServerId) {
		self.voice_handles.remove(&server_id);
	}

	/// Receive an event over the websocket, blocking until one is available.
	pub fn recv_event(&mut self) -> Result<Event> {
		match self.receiver.recv_json(GatewayEvent::decode) {
			Err(Error::WebSocket(err)) => {
				warn!("Websocket error, reconnecting: {:?}", err);
				// Try resuming if we haven't received an InvalidateSession
				if let Some(session_id) = self.session_id.clone() {
					match self.resume(session_id) {
						Ok(event) => return Ok(event),
						Err(e) => debug!("Failed to resume: {:?}", e),
					}
				}
				self.reconnect().map(Event::Ready)
			}
			Err(Error::Closed(num, message)) => {
				warn!("Closure, reconnecting: {:?}: {}", num, String::from_utf8_lossy(&message));
				// Try resuming if we haven't received a 1000, a 4006, or an InvalidateSession
				if num != Some(1000) && num != Some(4006) {
					if let Some(session_id) = self.session_id.clone() {
						match self.resume(session_id) {
							Ok(event) => return Ok(event),
							Err(e) => debug!("Failed to resume: {:?}", e),
						}
					}
				}
				self.reconnect().map(Event::Ready)
			}
			Err(error) => Err(error),
			Ok(GatewayEvent::Hello(interval)) => {
				debug!("Mysterious late-game hello: {}", interval);
				self.recv_event()
			}
			Ok(GatewayEvent::Dispatch(sequence, event)) => {
				self.last_sequence = sequence;
				let _ = self.keepalive_channel.send(Status::Sequence(sequence));
				if let Event::Resumed { heartbeat_interval, .. } = event {
					debug!("Resumed successfully");
					let _ = self.keepalive_channel.send(Status::ChangeInterval(heartbeat_interval));
				}
				voice_only! {{
					if let Event::VoiceStateUpdate(server_id, ref voice_state) = event {
						self.voice(server_id).__update_state(voice_state);
					}
					if let Event::VoiceServerUpdate { server_id, ref endpoint, ref token } = event {
						self.voice(server_id).__update_server(endpoint, token);
					}
				}}
				Ok(event)
			}
			Ok(GatewayEvent::Heartbeat(sequence)) => {
				debug!("Heartbeat received with seq {}", sequence);
				let map = ObjectBuilder::new()
					.insert("op", 1)
					.insert("d", sequence)
					.unwrap();
				let _ = self.keepalive_channel.send(Status::SendMessage(map));
				self.recv_event()
			}
			Ok(GatewayEvent::HeartbeatAck) => {
				self.recv_event()
			}
			Ok(GatewayEvent::Reconnect) => {
				self.reconnect().map(Event::Ready)
			}
			Ok(GatewayEvent::InvalidateSession) => {
				debug!("Session invalidated, reidentifying");
				self.session_id = None;
				let _ = self.keepalive_channel.send(Status::SendMessage(identify(&self.token)));
				self.recv_event()
			}
		}
	}

	/// Reconnect after receiving an OP7 RECONNECT
	fn reconnect(&mut self) -> Result<ReadyEvent> {
		debug!("Reconnecting...");
		// Make two attempts on the current known gateway URL
		for _ in 0..2 {
			if let Ok((conn, ready)) = Connection::new(&self.ws_url, &self.token) {
				try!(::std::mem::replace(self, conn).shutdown());
				self.session_id = Some(ready.session_id.clone());
				return Ok(ready)
			}
			::sleep_ms(1000);
		}
		// If those fail, hit REST for a new endpoint
		let (conn, ready) = try!(::Discord {
			client: ::hyper::client::Client::new(),
			token: self.token.to_owned()
		}.connect());
		try!(::std::mem::replace(self, conn).shutdown());
		self.session_id = Some(ready.session_id.clone());
		Ok(ready)
	}

	/// Resume using our existing session
	fn resume(&mut self, session_id: String) -> Result<Event> {
		debug!("Resuming...");
		// close connection and re-establish
		try!(self.receiver.get_mut().get_mut().shutdown(::std::net::Shutdown::Both));
		let url = try!(build_gateway_url(&self.ws_url));
		let response = try!(try!(Client::connect(url)).send());
		try!(response.validate());
		let (mut sender, mut receiver) = response.begin().split();

		// send the resume request
		let resume = ObjectBuilder::new()
			.insert("op", 6)
			.insert_object("d", |o| o
				.insert("seq", self.last_sequence)
				.insert("token", &self.token)
				.insert("session_id", session_id)
			)
			.unwrap();
		try!(sender.send_json(&resume));

		// TODO: when Discord has implemented it, observe the RESUMING event here
		let first_event;
		loop {
			match try!(receiver.recv_json(GatewayEvent::decode)) {
				GatewayEvent::Dispatch(seq, event) => {
					if let Event::Ready(ReadyEvent { ref session_id, .. }) = event {
						self.session_id = Some(session_id.clone());
					}
					self.last_sequence = seq;
					first_event = event;
					break
				},
				GatewayEvent::InvalidateSession => {
					debug!("Session invalidated in resume, reidentifying");
					try!(sender.send_json(&identify(&self.token)));
				}
				other => {
					debug!("Unexpected event: {:?}", other);
					return Err(Error::Protocol("Unexpected event during resume"))
				}
			}
		}

		// switch everything to the new connection
		self.receiver = receiver;
		let _ = self.keepalive_channel.send(Status::ChangeSender(sender));
		Ok(first_event)
	}

	/// Cleanly shut down the websocket connection. Optional.
	pub fn shutdown(mut self) -> Result<()> {
		use websocket::{Sender as S};
		use std::io::Write;

		// Hacky horror: get the WebSocketStream from the Receiver and formally close it
		let stream = self.receiver.get_mut().get_mut();
		try!(Sender::new(stream.by_ref(), true)
			.send_message(&::websocket::message::Message::close_because(1000, "")));
		try!(stream.flush());
		try!(stream.shutdown(::std::net::Shutdown::Both));
		Ok(())
	}

	#[doc(hidden)]
	pub fn __download_members(&self, servers: &[ServerId]) {
		let msg = ObjectBuilder::new()
			.insert("op", 8)
			.insert_object("d", |o| o
				.insert_array("guild_id", |a| servers.iter().fold(a, |a, s| a.push(s.0)))
				.insert("query", "")
				.insert("limit", 0)
			)
			.unwrap();
		let _ = self.keepalive_channel.send(Status::SendMessage(msg));
	}

	#[doc(hidden)]
	pub fn __guild_sync(&self, servers: &[ServerId]) {
		let msg = ObjectBuilder::new()
			.insert("op", 12)
			.insert_array("d", |a| servers.iter().fold(a, |a, s| a.push(s.0)))
			.unwrap();
		let _ = self.keepalive_channel.send(Status::SendMessage(msg));
	}
}

fn identify(token: &str) -> serde_json::Value {
	ObjectBuilder::new()
		.insert("op", 2)
		.insert_object("d", |object| object
			.insert("token", token)
			.insert_object("properties", |object| object
				.insert("$os", ::std::env::consts::OS)
				.insert("$browser", "Discord library for Rust")
				.insert("$device", "discord-rs")
				.insert("$referring_domain", "")
				.insert("$referrer", "")
			)
			.insert("v", GATEWAY_VERSION)
		)
		.unwrap()
}

#[inline]
fn build_gateway_url(base: &str) -> Result<::websocket::client::request::Url> {
	::websocket::client::request::Url::parse(&format!("{}?v={}", base, GATEWAY_VERSION))
		.map_err(|_| Error::Other("Invalid gateway URL"))
}

fn keepalive(interval: u64, mut sender: Sender<WebSocketStream>, channel: mpsc::Receiver<Status>) {
	let mut timer = ::Timer::new(interval);
	let mut last_sequence = 0;

	'outer: loop {
		::sleep_ms(100);

		loop {
			match channel.try_recv() {
				Ok(Status::SendMessage(val)) => {
					match sender.send_json(&val) {
						Ok(()) => {},
						Err(e) => warn!("Error sending gateway message: {:?}", e)
					}
				},
				Ok(Status::Sequence(seq)) => {
					last_sequence = seq;
				},
				Ok(Status::ChangeInterval(interval)) => {
					timer = ::Timer::new(interval);
				},
				Ok(Status::ChangeSender(new_sender)) => {
					sender = new_sender;
				}
				Err(mpsc::TryRecvError::Empty) => break,
				Err(mpsc::TryRecvError::Disconnected) => break 'outer,
			}
		}

		if timer.check_tick() {
			let map = ObjectBuilder::new()
				.insert("op", 1)
				.insert("d", last_sequence)
				.unwrap();
			match sender.send_json(&map) {
				Ok(()) => {},
				Err(e) => warn!("Error sending gateway keeaplive: {:?}", e)
			}
		}
	}
	let _ = sender.get_mut().shutdown(::std::net::Shutdown::Both);
}
