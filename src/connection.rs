#[cfg(feature = "voice")]
use std::collections::HashMap;
use std::sync::mpsc;

use websocket::client::{Client, Receiver, Sender};
use websocket::stream::WebSocketStream;

use serde_json;

use internal::Status;
use model::*;
#[cfg(feature = "voice")]
use voice::VoiceConnection;
use {Error, ReceiverExt, Result, SenderExt};

const GATEWAY_VERSION: u64 = 6;

#[cfg(feature = "voice")]
macro_rules! finish_connection {
	($($name1:ident: $val1:expr),*; $($name2:ident: $val2:expr,)*) => { Connection {
		$($name1: $val1,)*
		$($name2: $val2,)*
	}}
}
#[cfg(not(feature = "voice"))]
macro_rules! finish_connection {
	($($name1:ident: $val1:expr),*; $($name2:ident: $val2:expr,)*) => { Connection {
		$($name1: $val1,)*
	}}
}

/// Websocket connection to the Discord servers.
pub struct Connection {
	keepalive_channel: mpsc::Sender<Status>,
	receiver: Receiver<WebSocketStream>,
	#[cfg(feature = "voice")]
	voice_handles: HashMap<Option<ServerId>, VoiceConnection>,
	#[cfg(feature = "voice")]
	user_id: UserId,
	ws_url: String,
	token: String,
	session_id: Option<String>,
	last_sequence: u64,
	shard_info: Option<[u8; 2]>,
}

impl Connection {
	/// Establish a connection to the Discord websocket servers.
	///
	/// Returns both the `Connection` and the `ReadyEvent` which is always the
	/// first event received and contains initial state information.
	///
	/// Usually called internally by `Discord::connect`, which provides both
	/// the token and URL and an optional user-given shard ID and total shard
	/// count.
	pub fn new(
		base_url: &str,
		token: &str,
		shard_info: Option<[u8; 2]>,
	) -> Result<(Connection, ReadyEvent)> {
		trace!("Gateway: {}", base_url);
		// establish the websocket connection
		let url = build_gateway_url(base_url)?;
		let response = Client::connect(url)?.send()?;
		response.validate()?;
		let (mut sender, mut receiver) = response.begin().split();

		// send the handshake
		let identify = identify(token, shard_info);
		sender.send_json(&identify)?;

		// read the Hello and spawn the keepalive thread
		let heartbeat_interval;
		match receiver.recv_json(GatewayEvent::decode)? {
			GatewayEvent::Hello(interval) => heartbeat_interval = interval,
			other => {
				debug!("Unexpected event: {:?}", other);
				return Err(Error::Protocol("Expected Hello during handshake"));
			}
		}

		let (tx, rx) = mpsc::channel();
		::std::thread::Builder::new()
			.name("Discord Keepalive".into())
			.spawn(move || keepalive(heartbeat_interval, sender, rx))?;

		// read the Ready event
		let sequence;
		let ready;
		match receiver.recv_json(GatewayEvent::decode)? {
			GatewayEvent::Dispatch(seq, Event::Ready(event)) => {
				sequence = seq;
				ready = event;
			}
			GatewayEvent::InvalidateSession => {
				debug!("Session invalidated, reidentifying");
				let _ = tx.send(Status::SendMessage(identify));
				match receiver.recv_json(GatewayEvent::decode)? {
					GatewayEvent::Dispatch(seq, Event::Ready(event)) => {
						sequence = seq;
						ready = event;
					}
					GatewayEvent::InvalidateSession => {
						return Err(Error::Protocol(
							"Invalid session during handshake. \
							Double-check your token or consider waiting 5 seconds between starting shards.",
						))
					}
					other => {
						debug!("Unexpected event: {:?}", other);
						return Err(Error::Protocol("Expected Ready during handshake"));
					}
				}
			}
			other => {
				debug!("Unexpected event: {:?}", other);
				return Err(Error::Protocol(
					"Expected Ready or InvalidateSession during handshake",
				));
			}
		}
		if ready.version != GATEWAY_VERSION {
			warn!(
				"Got protocol version {} instead of {}",
				ready.version, GATEWAY_VERSION
			);
		}
		let session_id = ready.session_id.clone();

		// return the connection
		Ok((
			finish_connection!(
				keepalive_channel: tx,
				receiver: receiver,
				ws_url: base_url.to_owned(),
				token: token.to_owned(),
				session_id: Some(session_id),
				last_sequence: sequence,
				shard_info: shard_info;
				// voice only
				user_id: ready.user.id,
				voice_handles: HashMap::new(),
			),
			ready,
		))
	}

	/// Change the game information that this client reports as playing.
	pub fn set_game(&self, game: Option<Game>) {
		self.set_presence(game, OnlineStatus::Online, false)
	}

	/// Set the client to be playing this game, with defaults used for any
	/// extended information.
	pub fn set_game_name(&self, name: String) {
		self.set_presence(Some(Game::playing(name)), OnlineStatus::Online, false);
	}

	/// Sets the active presence of the client, including game and/or status
	/// information.
	///
	/// `afk` will help Discord determine where to send notifications.
	pub fn set_presence(&self, game: Option<Game>, status: OnlineStatus, afk: bool) {
		let status = match status {
			OnlineStatus::Offline => OnlineStatus::Invisible,
			other => other,
		};
		let game = match game {
			Some(Game {
				kind: GameType::Streaming,
				url: Some(url),
				name,
			}) => json! {{ "type": GameType::Streaming, "url": url, "name": name }},
			Some(game) => json! {{ "name": game.name, "type": GameType::Playing }},
			None => json!(null),
		};
		let msg = json! {{
			"op": 3,
			"d": {
				"afk": afk,
				"since": 0,
				"status": status,
				"game": game,
			}
		}};
		let _ = self.keepalive_channel.send(Status::SendMessage(msg));
	}

	/// Get a handle to the voice connection for a server.
	///
	/// Pass `None` to get the handle for group and one-on-one calls.
	#[cfg(feature = "voice")]
	pub fn voice(&mut self, server_id: Option<ServerId>) -> &mut VoiceConnection {
		let Connection {
			ref mut voice_handles,
			user_id,
			ref keepalive_channel,
			..
		} = *self;
		voice_handles.entry(server_id).or_insert_with(|| {
			VoiceConnection::__new(server_id, user_id, keepalive_channel.clone())
		})
	}

	/// Drop the voice connection for a server, forgetting all settings.
	///
	/// Calling `.voice(server_id).disconnect()` will disconnect from voice but retain the mute
	/// and deaf status, audio source, and audio receiver.
	///
	/// Pass `None` to drop the connection for group and one-on-one calls.
	#[cfg(feature = "voice")]
	pub fn drop_voice(&mut self, server_id: Option<ServerId>) {
		self.voice_handles.remove(&server_id);
	}

	/// Receive an event over the websocket, blocking until one is available.
	pub fn recv_event(&mut self) -> Result<Event> {
		loop {
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
					// If resuming didn't work, reconnect
					return self.reconnect().map(Event::Ready);
				}
				Err(Error::Closed(num, message)) => {
					debug!("Closure, reconnecting: {:?}: {}", num, message);
					// Try resuming if we haven't received a 4006 or an InvalidateSession
					if num != Some(4006) {
						if let Some(session_id) = self.session_id.clone() {
							match self.resume(session_id) {
								Ok(event) => return Ok(event),
								Err(e) => debug!("Failed to resume: {:?}", e),
							}
						}
					}
					// If resuming didn't work, reconnect
					return self.reconnect().map(Event::Ready);
				}
				Err(error) => return Err(error),
				Ok(GatewayEvent::Hello(interval)) => {
					debug!("Mysterious late-game hello: {}", interval);
				}
				Ok(GatewayEvent::Dispatch(sequence, event)) => {
					self.last_sequence = sequence;
					let _ = self.keepalive_channel.send(Status::Sequence(sequence));
					#[cfg(feature = "voice")]
					{
						if let Event::VoiceStateUpdate(server_id, ref voice_state) = event {
							self.voice(server_id).__update_state(voice_state);
						}
						if let Event::VoiceServerUpdate {
							server_id,
							ref endpoint,
							ref token,
							..
						} = event
						{
							self.voice(server_id).__update_server(endpoint, token);
						}
					}
					return Ok(event);
				}
				Ok(GatewayEvent::Heartbeat(sequence)) => {
					debug!("Heartbeat received with seq {}", sequence);
					let map = json! {{
						"op": 1,
						"d": sequence,
					}};
					let _ = self.keepalive_channel.send(Status::SendMessage(map));
				}
				Ok(GatewayEvent::HeartbeatAck) => {}
				Ok(GatewayEvent::Reconnect) => {
					return self.reconnect().map(Event::Ready);
				}
				Ok(GatewayEvent::InvalidateSession) => {
					debug!("Session invalidated, reidentifying");
					self.session_id = None;
					let _ = self
						.keepalive_channel
						.send(Status::SendMessage(identify(&self.token, self.shard_info)));
				}
			}
		}
	}

	/// Reconnect after receiving an OP7 RECONNECT
	fn reconnect(&mut self) -> Result<ReadyEvent> {
		::sleep_ms(1000);
		self.keepalive_channel
			.send(Status::Aborted)
			.expect("Could not stop the keepalive thread, there will be a thread leak.");
		trace!("Reconnecting...");
		// Make two attempts on the current known gateway URL
		for _ in 0..2 {
			if let Ok((conn, ready)) = Connection::new(&self.ws_url, &self.token, self.shard_info) {
				::std::mem::replace(self, conn).raw_shutdown();
				self.session_id = Some(ready.session_id.clone());
				return Ok(ready);
			}
			::sleep_ms(1000);
		}
		// If those fail, hit REST for a new endpoint
		let (conn, ready) = ::Discord::from_token_raw(self.token.to_owned()).connect()?;
		::std::mem::replace(self, conn).raw_shutdown();
		self.session_id = Some(ready.session_id.clone());
		Ok(ready)
	}

	/// Resume using our existing session
	fn resume(&mut self, session_id: String) -> Result<Event> {
		::sleep_ms(1000);
		trace!("Resuming...");
		// close connection and re-establish
		self
			.receiver
			.get_mut()
			.get_mut()
			.shutdown(::std::net::Shutdown::Both)?;
		let url = build_gateway_url(&self.ws_url)?;
		let response = Client::connect(url)?.send()?;
		response.validate()?;
		let (mut sender, mut receiver) = response.begin().split();

		// send the resume request
		let resume = json! {{
			"op": 6,
			"d": {
				"seq": self.last_sequence,
				"token": self.token,
				"session_id": session_id,
			}
		}};
		sender.send_json(&resume)?;

		// TODO: when Discord has implemented it, observe the RESUMING event here
		let first_event;
		loop {
			match receiver.recv_json(GatewayEvent::decode)? {
				GatewayEvent::Hello(interval) => {
					let _ = self
						.keepalive_channel
						.send(Status::ChangeInterval(interval));
				}
				GatewayEvent::Dispatch(seq, event) => {
					if let Event::Resumed { .. } = event {
						trace!("Resumed successfully");
					}
					if let Event::Ready(ReadyEvent { ref session_id, .. }) = event {
						self.session_id = Some(session_id.clone());
					}
					self.last_sequence = seq;
					first_event = event;
					break;
				}
				GatewayEvent::InvalidateSession => {
					debug!("Session invalidated in resume, reidentifying");
					sender.send_json(&identify(&self.token, self.shard_info))?;
				}
				other => {
					debug!("Unexpected event: {:?}", other);
					return Err(Error::Protocol("Unexpected event during resume"));
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
		self.inner_shutdown()?;
		::std::mem::forget(self); // don't call a second time
		Ok(())
	}

	// called from shutdown() and drop()
	fn inner_shutdown(&mut self) -> Result<()> {
		use std::io::Write;
		use websocket::Sender as S;

		// Hacky horror: get the WebSocketStream from the Receiver and formally close it
		let stream = self.receiver.get_mut().get_mut();
		Sender::new(stream.by_ref(), true)
			.send_message(&::websocket::message::Message::close_because(1000, ""))?;
		stream.flush()?;
		stream.shutdown(::std::net::Shutdown::Both)?;
		self.keepalive_channel
			.send(Status::Aborted)
			.expect("Could not stop the keepalive thread, there will be a thread leak.");
		Ok(())
	}

	// called when we want to drop the connection with no fanfare
	fn raw_shutdown(mut self) {
		use std::io::Write;
		{
			let stream = self.receiver.get_mut().get_mut();
			let _ = stream.flush();
			let _ = stream.shutdown(::std::net::Shutdown::Both);
		}
		::std::mem::forget(self); // don't call inner_shutdown()
	}

	/// Requests a download of online member lists.
	///
	/// It is recommended to avoid calling this method until the online member list
	/// is actually needed, especially for large servers, in order to save bandwidth
	/// and memory.
	///
	/// Can be used with `State::all_servers`.
	pub fn sync_servers(&self, servers: &[ServerId]) {
		let msg = json! {{
			"op": 12,
			"d": servers,
		}};
		let _ = self.keepalive_channel.send(Status::SendMessage(msg));
	}

	/// Request a synchronize of active calls for the specified channels.
	///
	/// Can be used with `State::all_private_channels`.
	pub fn sync_calls(&self, channels: &[ChannelId]) {
		for &channel in channels {
			let msg = json! {{
				"op": 13,
				"d": { "channel_id": channel }
			}};
			let _ = self.keepalive_channel.send(Status::SendMessage(msg));
		}
	}

	/// Requests a download of all member information for large servers.
	///
	/// The members lists are cleared on call, and then refilled as chunks are received. When
	/// `unknown_members()` returns 0, the download has completed.
	pub fn download_all_members(&mut self, state: &mut ::State) {
		if state.unknown_members() == 0 {
			return;
		}
		let servers = state.__download_members();
		let msg = json! {{
			"op": 8,
			"d": {
				"guild_id": servers,
				"query": "",
				"limit": 0,
			}
		}};
		let _ = self.keepalive_channel.send(Status::SendMessage(msg));
	}
}

impl Drop for Connection {
	fn drop(&mut self) {
		// Swallow errors
		let _ = self.inner_shutdown();
	}
}

fn identify(token: &str, shard_info: Option<[u8; 2]>) -> serde_json::Value {
	let mut result = json! {{
		"op": 2,
		"d": {
			"token": token,
			"properties": {
				"$os": ::std::env::consts::OS,
				"$browser": "Discord library for Rust",
				"$device": "discord-rs",
				"$referring_domain": "",
				"$referrer": "",
			},
			"large_threshold": 250,
			"compress": true,
			"v": GATEWAY_VERSION,
		}
	}};
	if let Some(info) = shard_info {
		result["d"]["shard"] = json![[info[0], info[1]]];
	}
	result
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
				Ok(Status::SendMessage(val)) => match sender.send_json(&val) {
					Ok(()) => {}
					Err(e) => warn!("Error sending gateway message: {:?}", e),
				},
				Ok(Status::Sequence(seq)) => {
					last_sequence = seq;
				}
				Ok(Status::ChangeInterval(interval)) => {
					timer = ::Timer::new(interval);
				}
				Ok(Status::ChangeSender(new_sender)) => {
					sender = new_sender;
				}
				Ok(Status::Aborted) => break 'outer,
				Err(mpsc::TryRecvError::Empty) => break,
				Err(mpsc::TryRecvError::Disconnected) => break 'outer,
			}
		}

		if timer.check_tick() {
			let map = json! {{
				"op": 1,
				"d": last_sequence
			}};
			match sender.send_json(&map) {
				Ok(()) => {}
				Err(e) => warn!("Error sending gateway keeaplive: {:?}", e),
			}
		}
	}
	let _ = sender.get_mut().shutdown(::std::net::Shutdown::Both);
}
