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
		try!($opt.ok_or(Error::Other(stringify!($opt))))
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
	pub avatar: String,
}

impl User {
	pub fn decode(mut value: BTreeMap<String, Value>) -> Result<User> {
		Ok(User {
			id: UserId(req!(value.remove("id").and_then(into_string))),
			name: req!(value.remove("username").and_then(into_string)),
			discriminator: req!(value.remove("discriminator").and_then(into_string)),
			avatar: req!(value.remove("avatar").and_then(into_string)),
		})
	}
}

/*pub enum Channel {
	Private(PrivateChannel),
	Public(PublicChannel),
}*/

pub struct PrivateChannel {
	pub id: ChannelId,
	pub last_message_id: Option<MessageId>,
	pub recipient: User,
}

#[derive(Debug)]
pub struct Channel {
	pub id: ChannelId,
	pub name: String,
	pub server_id: ServerId,
	//pub is_private: bool,
	//pub permission_overwrites: (),
	pub topic: String,
	pub position: u64,
	//pub last_message_id: Option<MessageId>,
	pub kind: ChannelType,
}

impl Channel {
	pub fn decode(mut value: BTreeMap<String, Value>) -> Result<Channel> {
		Ok(Channel {
			id: req!(value.remove("id").and_then(into_string).map(ChannelId)),
			name: req!(value.remove("name").and_then(into_string)),
			server_id: req!(value.remove("server_id").and_then(into_string).map(ServerId)),
			//is_private: req!(req!(value.remove("is_private")).as_boolean()),
			topic: req!(value.remove("topic").and_then(into_string)),
			position: req!(req!(value.remove("position")).as_u64()),
			kind: req!(value.remove("topic").and_then(into_string).and_then(|s| ChannelType::from_name(&s))),
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
			id: MessageId(req!(value.remove("id").and_then(into_string))),
			channel_id: ChannelId(req!(value.remove("channel_id").and_then(into_string))),
			nonce: value.remove("nonce").and_then(into_string),
			content: req!(value.remove("content").and_then(into_string)),
			tts: req!(req!(value.remove("tts")).as_boolean()),
			timestamp: req!(value.remove("timestamp").and_then(into_string)),
			edited_timestamp: value.remove("edited_timestamp").and_then(into_string),
			mention_everyone: req!(req!(value.remove("mention_everyone")).as_boolean()),
			mentions: try!(req!(into_array(req!(value.remove("mentions")))).into_iter()
				.map(|v| into_string(v).ok_or(Error::Other("mentions")).map(UserId)).collect()),
			author: try!(User::decode(req!(value.remove("author").and_then(into_map)))),
		})
	}
}

fn into_string(value: Value) -> Option<String> {
	match value {
		Value::String(s) => Some(s),
		_ => None,
	}
}

fn into_array(value: Value) -> Option<Vec<Value>> {
	match value {
		Value::Array(v) => Some(v),
		_ => None,
	}
}

fn into_map(value: Value) -> Option<BTreeMap<String, Value>> {
	match value {
		Value::Object(m) => Some(m),
		_ => None,
	}
}
