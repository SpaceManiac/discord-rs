#[cfg(feature="voice")]
use std::collections::HashMap;

use serde_json;
use async;

use model::*;
use internal::Status;
#[cfg(feature="voice")]
use voice::VoiceConnection;
use {Result, Error};

/// Websocket connection to the Discord servers.
pub struct Connection {
	inner: async::Connection,
	ws_url: String,
	token: String,
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
	pub fn new(base_url: &str, token: &str, shard_info: Option<[u8; 2]>) -> Result<(Connection, ReadyEvent)> {
		unimplemented!()
	}

}
/*
pub struct Connection {
	
	keepalive_channel: mpsc::Sender<Status>,
	receiver: Receiver<WebSocketStream>,
	#[cfg(feature="voice")]
	voice_handles: HashMap<Option<ServerId>, VoiceConnection>,
	#[cfg(feature="voice")]
	user_id: UserId,

	session_id: Option<String>,
	last_sequence: u64,
	shard_info: Option<[u8; 2]>,
	
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
			Some(Game {kind: GameType::Streaming, url: Some(url), name}) => json! {{ "type": GameType::Streaming, "url": url, "name": name }},
			Some(game) => json! {{ "name": game.name }},
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
	#[cfg(feature="voice")]
	pub fn voice(&mut self, server_id: Option<ServerId>) -> &mut VoiceConnection {
		let Connection { ref mut voice_handles, user_id, ref keepalive_channel, .. } = *self;
		voice_handles.entry(server_id).or_insert_with(||
			VoiceConnection::__new(server_id, user_id, keepalive_channel.clone())
		)
	}

	/// Drop the voice connection for a server, forgetting all settings.
	///
	/// Calling `.voice(server_id).disconnect()` will disconnect from voice but retain the mute
	/// and deaf status, audio source, and audio receiver.
	///
	/// Pass `None` to drop the connection for group and one-on-one calls.
	#[cfg(feature="voice")]
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
					warn!("Closure, reconnecting: {:?}: {}", num, message);
					// Try resuming if we haven't received a 1000, a 4006, or an InvalidateSession
					if num != Some(1000) && num != Some(4006) {
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
					#[cfg(feature="voice")] {
						if let Event::VoiceStateUpdate(server_id, ref voice_state) = event {
							self.voice(server_id).__update_state(voice_state);
						}
						if let Event::VoiceServerUpdate { server_id, channel_id: _, ref endpoint, ref token } = event {
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
				Ok(GatewayEvent::HeartbeatAck) => {
				}
				Ok(GatewayEvent::Reconnect) => {
					return self.reconnect().map(Event::Ready);
				}
				Ok(GatewayEvent::InvalidateSession) => {
					debug!("Session invalidated, reidentifying");
					self.session_id = None;
					let _ = self.keepalive_channel.send(Status::SendMessage(identify(&self.token, self.shard_info)));
				}
			}
		}
	}

	/// Reconnect after receiving an OP7 RECONNECT
	fn reconnect(&mut self) -> Result<ReadyEvent> {
		::sleep_ms(1000);
		debug!("Reconnecting...");
		// Make two attempts on the current known gateway URL
		for _ in 0..2 {
			if let Ok((conn, ready)) = Connection::new(&self.ws_url, &self.token, self.shard_info) {
				::std::mem::replace(self, conn).raw_shutdown();
				self.session_id = Some(ready.session_id.clone());
				return Ok(ready)
			}
			::sleep_ms(1000);
		}
		// If those fail, hit REST for a new endpoint
		let (conn, ready) = try!(::Discord::from_token_raw(self.token.to_owned()).connect());
		::std::mem::replace(self, conn).raw_shutdown();
		self.session_id = Some(ready.session_id.clone());
		Ok(ready)
	}

	/// Resume using our existing session
	fn resume(&mut self, session_id: String) -> Result<Event> {
		::sleep_ms(1000);
		debug!("Resuming...");
		// close connection and re-establish
		try!(self.receiver.get_mut().get_mut().shutdown(::std::net::Shutdown::Both));
		let url = try!(build_gateway_url(&self.ws_url));
		let response = try!(try!(Client::connect(url)).send());
		try!(response.validate());
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
		try!(sender.send_json(&resume));

		// TODO: when Discord has implemented it, observe the RESUMING event here
		let first_event;
		loop {
			match try!(receiver.recv_json(GatewayEvent::decode)) {
				GatewayEvent::Hello(interval) => {
					let _ = self.keepalive_channel.send(Status::ChangeInterval(interval));
				}
				GatewayEvent::Dispatch(seq, event) => {
					if let Event::Resumed { .. } = event {
						debug!("Resumed successfully");
					}
					if let Event::Ready(ReadyEvent { ref session_id, .. }) = event {
						self.session_id = Some(session_id.clone());
					}
					self.last_sequence = seq;
					first_event = event;
					break
				},
				GatewayEvent::InvalidateSession => {
					debug!("Session invalidated in resume, reidentifying");
					try!(sender.send_json(&identify(&self.token, self.shard_info)));
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
		try!(self.inner_shutdown());
		::std::mem::forget(self); // don't call a second time
		Ok(())
	}

	// called from shutdown() and drop()
	fn inner_shutdown(&mut self) -> Result<()> {
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
		if state.unknown_members() == 0 { return }
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
} */

