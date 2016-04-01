use std::sync::mpsc;
use std::collections::HashMap;

use websocket::client::{Client, Sender, Receiver};
use websocket::stream::WebSocketStream;

use serde_json;
use serde_json::builder::ObjectBuilder;

use model::*;
use internal::Status;
use voice::VoiceConnection;
use {Result, Error, SenderExt, ReceiverExt};

/// Websocket connection to the Discord servers.
pub struct Connection {
	keepalive_channel: mpsc::Sender<Status>,
	receiver: Receiver<WebSocketStream>,
	voice_handles: HashMap<ServerId, VoiceConnection>,
	user_id: UserId,
	token: String,
}

impl Connection {
	/// Establish a connection to the Discord websocket servers.
	///
	/// Returns both the `Connection` and the `ReadyEvent` which is always the
	/// first event received and contains initial state information.
	///
	/// Usually called internally by `Discord::connect`, which provides both
	/// the token and URL.
	pub fn new(url: &str, token: &str) -> Result<(Connection, ReadyEvent)> {
		debug!("Gateway: {}", url);
		// establish the websocket connection
		let url = match ::websocket::client::request::Url::parse(url) {
			Ok(url) => url,
			Err(_) => return Err(Error::Other("Invalid URL in Connection::new()"))
		};
		let response = try!(try!(Client::connect(url)).send());
		try!(response.validate());
		let (mut sender, mut receiver) = response.begin().split();

		// send the handshake
		let map = ObjectBuilder::new()
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
				.insert("v", ::GATEWAY_VERSION)
			)
			.unwrap();
		try!(sender.send_json(&map));

		// read the Ready event
		let event = try!(receiver.recv_json(Event::decode));
		let ready = match event {
			Event::Ready(ready) => ready,
			_ => return Err(Error::Protocol("First event was not Ready"))
		};
		let heartbeat_interval = ready.heartbeat_interval;

		// spawn the keepalive thread
		let (tx, rx) = mpsc::channel();
		try!(::std::thread::Builder::new()
			.name("Discord Keepalive".into())
			.spawn(move || keepalive(heartbeat_interval, sender, rx)));

		// return the connection
		Ok((Connection {
			keepalive_channel: tx,
			receiver: receiver,
			voice_handles: HashMap::new(),
			user_id: ready.user.id,
			token: token.to_owned(),
		}, ready))
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
		self.set_game(Some(Game { name: name }));
	}

	/// Get a handle to the voice connection for a server.
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
	pub fn drop_voice(&mut self, server_id: ServerId) {
		self.voice_handles.remove(&server_id);
	}

	/// Receive an event over the websocket, blocking until one is available.
	pub fn recv_event(&mut self) -> Result<Event> {
		match self.receiver.recv_json(Event::decode) {
			Ok(Event::_ChangeGateway(url)) => {
				let (conn, ready) = try!(Connection::new(&url, &self.token));
				try!(::std::mem::replace(self, conn).shutdown());
				Ok(Event::GatewayChanged(url, ready))
			}
			Ok(Event::VoiceStateUpdate(server_id, voice_state)) => {
				self.voice(server_id).__update_state(&voice_state);
				Ok(Event::VoiceStateUpdate(server_id, voice_state))
			}
			Ok(Event::VoiceServerUpdate { server_id, endpoint, token }) => {
				self.voice(server_id).__update_server(&endpoint, &token);
				Ok(Event::VoiceServerUpdate { server_id: server_id, endpoint: endpoint, token: token })
			}
			other => other
		}
	}

	/// Cleanly shut down the websocket connection. Optional.
	pub fn shutdown(mut self) -> Result<()> {
		try!(self.receiver.get_mut().get_mut().shutdown(::std::net::Shutdown::Both));
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
}

fn keepalive(interval: u64, mut sender: Sender<WebSocketStream>, channel: mpsc::Receiver<Status>) {
	let mut timer = ::Timer::new(interval);

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
				Err(mpsc::TryRecvError::Empty) => break,
				Err(mpsc::TryRecvError::Disconnected) => break 'outer,
			}
		}

		if timer.check_tick() {
			let map = ObjectBuilder::new()
				.insert("op", 1)
				.insert("d", ::time::get_time().sec)
				.unwrap();
			match sender.send_json(&map) {
				Ok(()) => {},
				Err(e) => warn!("Error sending gateway keeaplive: {:?}", e)
			}
		}
	}
	let _ = sender.get_mut().shutdown(::std::net::Shutdown::Both);
}
