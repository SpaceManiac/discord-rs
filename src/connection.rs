use super::{Result, Error};

use std::sync::mpsc;

use websocket::ws::sender::Sender as SenderTrait;
use websocket::client::{Client, Sender, Receiver};
use websocket::stream::WebSocketStream;
use websocket::message::{Message as WsMessage, Type as MessageType};

use serde_json;
use serde_json::builder::ObjectBuilder;

use super::model::*;

/// The websocket protocol version expected.
const VERSION: u64 = 3;

/// Websocket connection to the Discord servers.
pub struct Connection {
	keepalive_channel: mpsc::Sender<Status>,
	receiver: Receiver<WebSocketStream>,
	ready_event: Option<Event>,
	/// Known state composed from received events
	pub state: State,
}

impl Connection {
	pub fn new(url: &str, token: &str) -> Result<Connection> {
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
					.insert("$browser", "Howl library for Rust")
					.insert("$device", "howl")
					.insert("$referring_domain", "")
					.insert("$referrer", "")
				)
				.insert("v", 3)
			)
			.unwrap();
		try!(sender.send_message(&WsMessage::text(try!(serde_json::to_string(&map)))));

		// read the Ready event
		let ready = try!(recv_message(&mut receiver));
		let state = try!(State::new(ready.clone()));
		let heartbeat_interval = state.heartbeat_interval;

		// spawn the keepalive thread
		let (tx, rx) = mpsc::channel();
		::std::thread::spawn(move || keepalive(heartbeat_interval, sender, rx));

		// return the connection
		Ok(Connection {
			keepalive_channel: tx,
			receiver: receiver,
			ready_event: Some(ready),
			state: state,
		})
	}

	pub fn set_game_id(&mut self, game_id: Option<u64>) {
		let _ = self.keepalive_channel.send(Status::SetGameId(game_id));
	}

	pub fn recv_event(&mut self) -> Result<Event> {
		// clear the ready event
		if let Some(ready) = self.ready_event.take() {
			Ok(ready)
		} else {
			match recv_message(&mut self.receiver) {
				Err(err) => Err(err),
				Ok(event) => {
					self.state.update(&event);
					Ok(event)
				}
			}
		}
	}

	pub fn shutdown(&mut self) -> Result<()> {
		let _ = self.keepalive_channel.send(Status::Shutdown);
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
		println!("Unexpected message: {:?}", message);
		Ok(Event::Closed(0xfffe))
	} else {
		let json: serde_json::Value = try!(serde_json::from_reader(&message.payload[..]));
		let original = format!("{:?}", json);
		match Event::decode(json) {
			Ok(event) => Ok(event),
			Err(err) => {
				// If there was a decode failure, print the original json for debugging
				println!("<<< {} >>>", original);
				Err(err)
			}
		}
	}
}

enum Status {
	Shutdown,
	SetGameId(Option<u64>),
}

fn keepalive(interval: u64, mut sender: Sender<WebSocketStream>, channel: mpsc::Receiver<Status>) {
	let mut countdown = interval;
	let mut game_id = None;
	'outer: loop {
		// TODO: this is not a precise timer, but it's good enough for now
		::std::thread::sleep_ms(100);
		countdown = countdown.saturating_sub(100);

		loop {
			match channel.try_recv() {
				Ok(Status::Shutdown) => break 'outer,
				Ok(Status::SetGameId(id)) => { game_id = id; countdown = 0; },
				Err(mpsc::TryRecvError::Empty) => break,
				Err(mpsc::TryRecvError::Disconnected) => break 'outer,
			}
		}

		if countdown == 0 {
			countdown = interval;
			let map = ObjectBuilder::new()
				.insert("op", 3)
				.insert_object("d", |object| object
					.insert("idle_since", serde_json::Value::Null)
					.insert("game_id", game_id)
				)
				.unwrap();
			let json = match serde_json::to_string(&map) {
				Ok(json) => json,
				Err(e) => return println!("error jsoning ping: {:?}", e)
			};
			match sender.send_message(&WsMessage::text(json)) {
				Ok(()) => {},
				Err(e) => return println!("error sending ping: {:?}", e)
			}
		}
	}
	let _ = sender.get_mut().shutdown(::std::net::Shutdown::Both);
}

/// State tracking for events received over Discord.
pub struct State {
	user: SelfInfo,
	session_id: String,
	heartbeat_interval: u64,
	private_channels: Vec<PrivateChannel>,
	servers: Vec<ServerInfo>,
}

impl State {
	fn new(ready: Event) -> Result<State> {
		match ready {
			Event::Ready { version, user, session_id, heartbeat_interval, private_channels, servers, read_state: _ } => {
				if version != VERSION {
					println!("[Warning] Got version {} instead of {}", version, VERSION);
					return Err(Error::Other("Wrong version specified"))
				}
				Ok(State {
					user: user,
					session_id: session_id,
					heartbeat_interval: heartbeat_interval,
					private_channels: private_channels,
					servers: servers,
				})
			},
			_ => Err(Error::Other("First event for State must be Ready")),
		}
	}

	fn update(&mut self, event: &Event) {
		match *event {
			Event::UserUpdate(ref user) => self.user = user.clone(),
			Event::VoiceStateUpdate(ref server, ref state) => {
				self.servers.iter_mut().find(|s| s.id == *server).map(|srv| {
					match srv.voice_states.iter_mut().find(|u| u.user_id == state.user_id) {
						Some(srv_state) => { srv_state.clone_from(state); return }
						None => {}
					}
					srv.voice_states.push(state.clone());
				});
			}
			Event::PresenceUpdate { ref server_id, ref presence, roles: _ } => {
				// TODO: double-check this
				self.servers.iter_mut().find(|s| s.id == *server_id).map(|srv| {
					match srv.presences.iter_mut().find(|u| u.user_id == presence.user_id) {
						Some(srv_presence) => { srv_presence.clone_from(presence); return }
						None => {}
					}
					srv.presences.push(presence.clone());
				});
			}
			Event::ServerCreate(ref server) => self.servers.push(server.clone()),
			Event::ServerUpdate(ref server) => {
				self.servers.iter_mut().find(|s| s.id == server.id).map(|srv| {
					srv.name.clone_from(&server.name);
					srv.joined_at.clone_from(&server.joined_at);
					srv.afk_timeout = server.afk_timeout;
					srv.afk_channel_id.clone_from(&server.afk_channel_id);
					srv.icon.clone_from(&server.icon);
					srv.roles.clone_from(&server.roles);
					srv.region.clone_from(&server.region);
					// embed_enabled and embed_channel_id skipped
					srv.owner_id.clone_from(&server.owner_id);
				});
			}
			Event::ServerDelete(ref server) => self.servers.retain(|s| s.id != server.id),
			Event::ServerMemberAdd { ref server_id, ref joined_at, ref roles, ref user } => {
				self.servers.iter_mut().find(|s| s.id == *server_id).map(|srv| {
					srv.members.push(Member {
						user: user.clone(),
						roles: roles.clone(),
						joined_at: joined_at.clone(),
						mute: false,
						deaf: false,
					})
				});
			}
			Event::ServerMemberUpdate { ref server_id, ref roles, ref user } => {
				self.servers.iter_mut().find(|s| s.id == *server_id).map(|srv| {
					srv.members.iter_mut().find(|m| m.user.id == user.id).map(|member| {
						member.user.clone_from(user);
						member.roles.clone_from(roles);
					})
				});
			}
			Event::ServerMemberRemove(ref server_id, ref user) => {
				self.servers.iter_mut().find(|s| s.id == *server_id).map(|srv| {
					srv.members.retain(|m| m.user.id != user.id);
				});
			}
			Event::ServerRoleCreate(ref server_id, ref role) => {
				self.servers.iter_mut().find(|s| s.id == *server_id).map(|srv| {
					srv.roles.push(role.clone());
				});
			}
			Event::ServerRoleUpdate(ref server_id, ref role) => {
				self.servers.iter_mut().find(|s| s.id == *server_id).map(|srv| {
					srv.roles.iter_mut().find(|r| r.id == role.id).map(|srv_role| {
						srv_role.clone_from(role);
					});
				});
			}
			Event::ServerRoleDelete(ref server_id, ref role_id) => {
				self.servers.iter_mut().find(|s| s.id == *server_id).map(|srv| {
					srv.roles.retain(|r| r.id != *role_id);
				});
			}
			Event::ChannelCreate(ref channel) => match *channel {
				Channel::Private(ref channel) => {
					self.private_channels.push(channel.clone());
				}
				Channel::Public(ref channel) => {
					self.servers.iter_mut().find(|s| s.id == channel.server_id).map(|srv| {
						srv.channels.push(channel.clone());
					});
				}
			},
			Event::ChannelUpdate(ref channel) => match *channel {
				Channel::Private(ref channel) => {
					self.private_channels.iter_mut().find(|c| c.id == channel.id).map(|chan| {
						chan.clone_from(channel);
					});
				}
				Channel::Public(ref channel) => {
					self.servers.iter_mut().find(|s| s.id == channel.server_id).map(|srv| {
						srv.channels.iter_mut().find(|c| c.id == channel.id).map(|chan| {
							chan.clone_from(channel);
						})
					});
				}
			},
			Event::ChannelDelete(ref channel) => match *channel {
				Channel::Private(ref channel) => {
					self.private_channels.retain(|c| c.id != channel.id);
				}
				Channel::Public(ref channel) => {
					self.servers.iter_mut().find(|s| s.id == channel.server_id).map(|srv| {
						srv.channels.retain(|c| c.id != channel.id);
					});
				}
			},
			_ => {}
		}
	}

	#[inline]
	pub fn user_info(&self) -> &SelfInfo { &self.user }

	#[inline]
	pub fn session_id(&self) -> &str { &self.session_id }

	#[inline]
	pub fn private_channels(&self) -> &[PrivateChannel] { &self.private_channels }

	#[inline]
	pub fn servers(&self) -> &[ServerInfo] { &self.servers }

	pub fn find_channel(&self, id: &ChannelId) -> Option<ChannelRef> {
		for server in &self.servers {
			for channel in &server.channels {
				if channel.id == *id {
					return Some(ChannelRef::Public(server, channel))
				}
			}
		}
		for channel in &self.private_channels {
			if channel.id == *id {
				return Some(ChannelRef::Private(channel))
			}
		}
		None
	}
}

pub enum ChannelRef<'a> {
	Public(&'a ServerInfo, &'a PublicChannel),
	Private(&'a PrivateChannel),
}
