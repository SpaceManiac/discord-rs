use super::{Result, Error};

use std::sync::mpsc;
use std::collections::HashMap;

use websocket::ws::sender::Sender as SenderTrait;
use websocket::client::{Client, Sender, Receiver};
use websocket::stream::WebSocketStream;
use websocket::message::{Message as WsMessage, Type as MessageType};

use serde_json;
use serde_json::builder::ObjectBuilder;

use model::*;
use internal::Status;
use voice::VoiceConnection;

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
		try!(sender.send_message(&WsMessage::text(try!(serde_json::to_string(&map)))));

		// read the Ready event
		let event = try!(recv_message(&mut receiver));
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
		let _ = self.keepalive_channel.send(Status::SetGame(game));
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
		match recv_message(&mut self.receiver) {
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
}

fn recv_message(receiver: &mut Receiver<WebSocketStream>) -> Result<Event> {
	use websocket::ws::receiver::Receiver;
	let message: WsMessage = try!(receiver.recv_message());
	if message.opcode == MessageType::Close {
		Ok(Event::Closed(message.cd_status_code.unwrap_or(0xffff)))
	} else if message.opcode != MessageType::Text {
		warn!("Unexpected message: {:?}", message);
		Ok(Event::Closed(0xfffe))
	} else {
		let json: serde_json::Value = try!(serde_json::from_reader(&message.payload[..]));
		let original = format!("{:?}", json);
		match Event::decode(json) {
			Ok(event) => Ok(event),
			Err(err) => {
				// If there was a decode failure, print the original json for debugging
				warn!("Error decoding: {}", original);
				Err(err)
			}
		}
	}
}

fn keepalive(interval: u64, mut sender: Sender<WebSocketStream>, channel: mpsc::Receiver<Status>) {
	let mut game = None;
	let mut timer = ::Timer::new(interval);

	'outer: loop {
		::sleep_ms(100);

		loop {
			match channel.try_recv() {
				Ok(Status::SetGame(new_game)) => {
					game = new_game;
					timer.immediately();
				},
				Ok(Status::SendMessage(val)) => {
					let json = match serde_json::to_string(&val) {
						Ok(json) => json,
						Err(e) => return warn!("Error encoding message: {:?}", e)
					};
					match sender.send_message(&WsMessage::text(json)) {
						Ok(()) => {},
						Err(e) => return warn!("Error sending message: {:?}", e)
					}
				},
				Err(mpsc::TryRecvError::Empty) => break,
				Err(mpsc::TryRecvError::Disconnected) => break 'outer,
			}
		}

		if timer.check_tick() {
			let map = ObjectBuilder::new()
				.insert("op", 3)
				.insert_object("d", |mut object| {
					object = object.insert("idle_since", serde_json::Value::Null);
					match game {
						Some(ref game) => object.insert_object("game", |object| object
							.insert("name", &game.name)
						),
						None => object.insert("game", serde_json::Value::Null),
					}
				})
				.unwrap();
			let json = match serde_json::to_string(&map) {
				Ok(json) => json,
				Err(e) => return warn!("Error encoding keepalive: {:?}", e)
			};
			match sender.send_message(&WsMessage::text(json)) {
				Ok(()) => {},
				Err(e) => return warn!("Error sending keepalive: {:?}", e)
			}
		}
	}
	let _ = sender.get_mut().shutdown(::std::net::Shutdown::Both);
}
