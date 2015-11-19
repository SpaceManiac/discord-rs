//! Model types.

use super::{Error, Result};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Clone, Hash, Eq, PartialEq, Debug, Ord, PartialOrd)]
pub struct UserId(pub String);

#[derive(Clone, Hash, Eq, PartialEq, Debug, Ord, PartialOrd)]
pub struct ServerId(pub String);

#[derive(Clone, Hash, Eq, PartialEq, Debug, Ord, PartialOrd)]
pub struct ChannelId(pub String);

#[derive(Clone, Hash, Eq, PartialEq, Debug, Ord, PartialOrd)]
pub struct MessageId(pub String);

#[derive(Clone, Hash, Eq, PartialEq, Debug, Ord, PartialOrd)]
pub struct RoleId(pub String);

#[derive(Copy, Clone, Hash, Eq, PartialEq, Debug)]
pub enum ChannelType {
	Text,
	Voice,
}

impl ChannelType {
	pub fn from_name(name: &str) -> Option<ChannelType> {
		if name == "text" {
			Some(ChannelType::Text)
		} else if name == "voice" {
			Some(ChannelType::Voice)
		} else {
			None
		}
	}
	pub fn name(&self) -> &'static str {
		match *self {
			ChannelType::Text => "text",
			ChannelType::Voice => "voice",
		}
	}
}

macro_rules! req {
	($opt:expr) => {
		try!($opt.ok_or(Error::Other(concat!(file!(), ":", line!(), ": ", stringify!($opt)))))
	}
}

#[derive(Debug)]
pub struct Server {
	pub id: ServerId,
	pub name: String,
	pub afk_timeout: u64,
	pub joined_at: String, // Timestamp
	pub afk_channel_id: Option<ChannelId>,
	//pub icon: Option<()>,
	pub roles: Vec<Role>,
	pub region: String,
	pub embed_enabled: bool,
	pub embed_channel_id: Option<ChannelId>,
	pub owner_id: UserId,
}

#[derive(Debug)]
pub struct Role {
	pub id: RoleId,
	pub name: String,
	pub color: u64,
	pub hoist: bool,
	pub managed: bool,
	pub position: i64,
	pub permissions: u64, // bitflags?
}

#[derive(Debug)]
pub struct User {
	pub id: UserId,
	pub name: String,
	pub discriminator: String,
	pub avatar: Option<String>,
}

fn remove(map: &mut BTreeMap<String, Value>, key: &'static str) -> Result<Value> {
	map.remove(key).ok_or(Error::Other(key))
}

fn decode_discriminator(value: Value) -> Result<String> {
	match into_string(value) {
		Ok(text) => Ok(text),
		Err(Error::Decode(_, value)) => match value.as_u64() {
			Some(num) => Ok(format!("{}", num)),
			None => Err(Error::Decode("string or u64", value))
		},
		Err(other) => Err(other),
	}
}

fn optional<T>(x: Option<Result<T>>) -> Result<Option<T>> {
	match x {
		Some(Ok(val)) => Ok(Some(val)),
		Some(Err(err)) => Err(err),
		None => Ok(None),
	}
}

impl User {
	pub fn decode(value: Value) -> Result<User> {
		let mut value = try!(into_map(value));
		Ok(User {
			id: try!(remove(&mut value, "id").and_then(into_string).map(UserId)),
			name: try!(remove(&mut value, "username").and_then(into_string)),
			discriminator: try!(remove(&mut value, "discriminator").and_then(decode_discriminator)),
			avatar: try!(optional(value.remove("avatar").map(into_string))),
		})
	}
}

#[derive(Debug)]
pub struct Member {
	pub user: User,
	pub roles: Vec<RoleId>,
	pub joined_at: String,
	pub mute: bool,
	pub deaf: bool,
}

impl Member {
	pub fn decode(value: Value) -> Result<Member> {
		let mut value = try!(into_map(value));
		Ok(Member {
			user: try!(remove(&mut value, "user").and_then(User::decode)),
			roles: try!(decode_array(try!(remove(&mut value, "roles")), |v| into_string(v).map(RoleId))),
			joined_at: try!(remove(&mut value, "joined_at").and_then(into_string)),
			mute: req!(try!(remove(&mut value, "mute")).as_boolean()),
			deaf: req!(try!(remove(&mut value, "deaf")).as_boolean()),
		})
	}
}

#[derive(Debug)]
pub enum Channel {
	Private(PrivateChannel),
	Public(PublicChannel),
}

impl Channel {
	pub fn decode(value: Value) -> Result<Channel> {
		let mut value = try!(into_map(value));
		if req!(req!(value.remove("is_private")).as_boolean()) {
			PrivateChannel::decode(Value::Object(value)).map(Channel::Private)
		} else {
			PublicChannel::decode(Value::Object(value)).map(Channel::Public)
		}
	}
}

#[derive(Debug)]
pub struct PrivateChannel {
	pub id: ChannelId,
	pub recipient: User,
	pub last_message_id: Option<MessageId>,
}

impl PrivateChannel {
	pub fn decode(value: Value) -> Result<PrivateChannel> {
		let mut value = try!(into_map(value));
		Ok(PrivateChannel {
			id: try!(remove(&mut value, "id").and_then(into_string).map(ChannelId)),
			recipient: try!(remove(&mut value, "recipient").and_then(User::decode)),
			last_message_id: try!(optional(value.remove("last_message_id").map(|v| into_string(v).map(MessageId)))),
		})
	}
}

#[derive(Debug)]
pub struct PublicChannel {
	pub id: ChannelId,
	pub name: String,
	pub server_id: ServerId,
	//pub permission_overwrites: (),
	pub topic: Option<String>,
	pub position: u64,
	pub last_message_id: Option<MessageId>,
	pub kind: ChannelType,
}

impl PublicChannel {
	pub fn decode(value: Value) -> Result<PublicChannel> {
		let mut value = try!(into_map(value));
		let id = try!(remove(&mut value, "server_id").and_then(into_string).map(ServerId));
		PublicChannel::decode_server(Value::Object(value), id)
	}

	pub fn decode_server(value: Value, server_id: ServerId) -> Result<PublicChannel> {
		let mut value = try!(into_map(value));
		Ok(PublicChannel {
			id: try!(remove(&mut value, "id").and_then(into_string).map(ChannelId)),
			name: try!(remove(&mut value, "name").and_then(into_string)),
			server_id: server_id,
			topic: try!(optional(value.remove("topic").map(into_string))),
			position: req!(try!(remove(&mut value, "position")).as_u64()),
			kind: try!(remove(&mut value, "type").and_then(into_string).and_then(|s| ChannelType::from_name(&s).ok_or(Error::Other("")))),
			last_message_id: try!(optional(value.remove("last_message_id").map(|v| into_string(v).map(MessageId)))),
		})
	}
}

#[derive(Debug)]
pub struct Message {
	pub id: MessageId,
	pub channel_id: ChannelId,
	pub content: String,
	pub nonce: Option<String>,
	pub tts: bool,
	pub timestamp: String,
	pub edited_timestamp: Option<String>,

	pub mention_everyone: bool,
	pub mentions: Vec<UserId>,
	
	pub author: User,

	//pub attachments: Vec<()>,
	//pub embeds: Vec<()>,
}

impl Message {
	pub fn decode(mut value: BTreeMap<String, Value>) -> Result<Message> {
		Ok(Message {
			id: try!(remove(&mut value, "id").and_then(into_string).map(MessageId)),
			channel_id: try!(remove(&mut value, "channel_id").and_then(into_string).map(ChannelId)),
			nonce: try!(optional(value.remove("nonce").map(into_string))),
			content: try!(remove(&mut value, "content").and_then(into_string)),
			tts: req!(try!(remove(&mut value, "tts")).as_boolean()),
			timestamp: try!(remove(&mut value, "timestamp").and_then(into_string)),
			edited_timestamp: try!(optional(value.remove("edited_timestamp").map(into_string))),
			mention_everyone: req!(try!(remove(&mut value, "mention_everyone")).as_boolean()),
			mentions: try!(decode_array(try!(remove(&mut value, "mentions")), |v| into_string(v).map(UserId))),
			author: try!(remove(&mut value, "author").and_then(User::decode)),
		})
	}
}

//=================
// Event stuff
/*
#[derive(Debug)]
pub struct ReadState {
	pub id: ChannelId,
	pub last_message_id: MessageId,
	pub mention_count: u64,
}

impl ReadState {
	pub fn decode(mut value: BTreeMap<String, Value>) -> Result<ReadState> {
		Ok(ReadState {
			id: req!(value.remove("id").and_then(into_string).map(ChannelId)),
			last_message_id: req!(value.remove("last_message_id").and_then(into_string).map(MessageId)),
			mention_count: req!(req!(value.remove("mention_count")).as_u64()),
		})
	}
}

#[derive(Debug)]
pub struct Presence {
	pub user_id: UserId,
	pub status: String, // enum?
	pub game_id: Option<u64>,
}

impl Presence {
	pub fn decode(mut value: BTreeMap<String, Value>) -> Result<Presence> {
		Ok(Presence {
			user_id: req!(req!(value.remove("user").and_then(into_map))
				.remove("id").and_then(into_string).map(UserId)),
			status: req!(value.remove("status").and_then(into_string)),
			game_id: value.remove("game_id").and_then(|x| x.as_u64()),
		})
	}
}

#[derive(Debug)]
pub struct VoiceState {
	pub user_id: UserId,
	pub channel_id: ChannelId,
	pub session_id: String,
	pub token: Option<String>,
	pub suppress: bool,
	pub self_mute: bool,
	pub self_deaf: bool,
	pub mute: bool,
	pub deaf: bool,
}

impl VoiceState {
	pub fn decode(mut value: BTreeMap<String, Value>) -> Result<VoiceState> {
		Ok(VoiceState {
			user_id: req!(value.remove("user_id").and_then(into_string).map(UserId)),
			channel_id: req!(value.remove("channel_id").and_then(into_string).map(ChannelId)),
			session_id: req!(value.remove("session_id").and_then(into_string)),
			token: value.remove("token").and_then(into_string),
			suppress: req!(req!(value.remove("suppress")).as_boolean()),
			self_mute: req!(req!(value.remove("self_mute")).as_boolean()),
			self_deaf: req!(req!(value.remove("self_deaf")).as_boolean()),
			mute: req!(req!(value.remove("mute")).as_boolean()),
			deaf: req!(req!(value.remove("deaf")).as_boolean()),
		})
	}
}

#[derive(Debug)]
pub struct RoleInfo {
	pub id: RoleId,
	pub name: String,
	pub permissions: u64,
}

impl RoleInfo {
	pub fn decode(mut value: BTreeMap<String, Value>) -> Result<RoleInfo> {
		Ok(RoleInfo {
			id: req!(value.remove("id").and_then(into_string).map(RoleId)),
			name: req!(value.remove("name").and_then(into_string)),
			permissions: req!(req!(value.remove("permissions")).as_u64()),
		})
	}
}

#[derive(Debug)]
pub struct ServerInfo {
	pub id: ServerId,
	pub name: String,
	pub owner_id: UserId,
	pub voice_states: Vec<VoiceState>,
	pub roles: Vec<RoleInfo>,
	pub region: String,
	pub presences: Vec<Presence>,
	pub members: Vec<Member>,
	pub joined_at: String,
	//icon: Option<()>,
	pub channels: Vec<PublicChannel>,
	pub afk_timeout: u64,
	pub afk_channel_id: Option<ChannelId>,
}

impl ServerInfo {
	pub fn decode(mut value: BTreeMap<String, Value>) -> Result<ServerInfo> {
		let id = req!(value.remove("id").and_then(into_string).map(ServerId));
		Ok(ServerInfo {
			name: req!(value.remove("name").and_then(into_string)),
			owner_id: req!(value.remove("owner_id").and_then(into_string).map(UserId)),
			voice_states: try!(decode_array(value.remove("voice_states"), VoiceState::decode)),
			roles: try!(decode_array(value.remove("roles"), RoleInfo::decode)),
			region: req!(value.remove("region").and_then(into_string)),
			presences: try!(decode_array(value.remove("presences"), Presence::decode)),
			members: try!(decode_array(value.remove("members"), Member::decode)),
			joined_at: req!(value.remove("joined_at").and_then(into_string)),
			afk_timeout: req!(req!(value.remove("afk_timeout")).as_u64()),
			afk_channel_id: value.remove("afk_channel_id").and_then(into_string).map(ChannelId),
			channels: try!(decode_array(value.remove("channels"), |v| PublicChannel::decode_server(v, id.clone()))),
			id: id,
		})
	}
}

#[derive(Debug)]
pub struct SelfInfo {
	pub id: UserId,
	pub username: String,
	pub discriminator: String,
	pub email: String,
	pub verified: bool,
	pub avatar: Option<String>,
}

impl SelfInfo {
	pub fn decode(mut value: BTreeMap<String, Value>) -> Result<SelfInfo> {
		Ok(SelfInfo {
			id: req!(value.remove("id").and_then(into_string).map(UserId)),
			username: req!(value.remove("username").and_then(into_string)),
			discriminator: req!(value.remove("discriminator").and_then(into_string)),
			email: req!(value.remove("email").and_then(into_string)),
			avatar: value.remove("avatar").and_then(into_string),
			verified: req!(req!(value.remove("verified")).as_boolean()),
		})
	}
}

#[derive(Debug)]
pub enum Event {
	Ready {
		user: SelfInfo,
		session_id: String,
		heartbeat_interval: u64,
		read_state: Vec<ReadState>,
		private_channels: Vec<PrivateChannel>,
		servers: Vec<ServerInfo>,
	},
	Closed(u16),
	Unknown
}

impl Event {
	pub fn decode(value: Value) -> Result<Event> {
		let mut value = try!(into_map(value));
		/*let op = req!(req!(value.remove("op")).as_u64());
		if op != 0 {
			Err(Error::Other("Nonzero opcode, TODO"))
		}*/
		let kind = req!(value.remove("t").and_then(into_string));
		let mut inner = req!(value.remove("d").and_then(into_map));
		if kind == "READY" {
			Ok(Event::Ready {
				user: try!(SelfInfo::decode(req!(inner.remove("user").and_then(into_map)))),
				session_id: req!(inner.remove("session_id").and_then(into_string)),
				heartbeat_interval: req!(req!(inner.remove("heartbeat_interval")).as_u64()),
				read_state: try!(decode_array(inner.remove("read_state"), ReadState::decode)),
				private_channels: try!(decode_array(inner.remove("private_channels"), PrivateChannel::decode)),
				servers: try!(decode_array(inner.remove("guilds"), ServerInfo::decode)),
			})
		} else {
			Ok(Event::Unknown)
		}
	}
}
*/
fn into_string(value: Value) -> Result<String> {
	match value {
		Value::String(s) => Ok(s),
		value => Err(Error::Decode("string", value)),
	}
}

fn into_array(value: Value) -> Result<Vec<Value>> {
	match value {
		Value::Array(v) => Ok(v),
		value => Err(Error::Decode("array", value)),
	}
}

fn into_map(value: Value) -> Result<BTreeMap<String, Value>> {
	match value {
		Value::Object(m) => Ok(m),
		value => Err(Error::Decode("object", value)),
	}
}

fn decode_array<T, F: Fn(Value) -> Result<T>>(value: Value, f: F) -> Result<Vec<T>> {
	into_array(value).and_then(|x| x.into_iter().map(f).collect())
}
