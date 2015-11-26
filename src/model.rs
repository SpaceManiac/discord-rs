//! Model types.

// TODO: When reading optional keys, type errors are silently treated as if the
// key was absent. Either decoding should fail or a warning should be printed.

use super::{Error, Result};
use serde_json::Value;
use std::collections::BTreeMap;

pub use self::permissions::Permissions;

/// An identifier for a User
#[derive(Clone, Hash, Eq, PartialEq, Debug, Ord, PartialOrd)]
pub struct UserId(pub String);

/// An identifier for a Server
#[derive(Clone, Hash, Eq, PartialEq, Debug, Ord, PartialOrd)]
pub struct ServerId(pub String);

/// An identifier for a Channel
#[derive(Clone, Hash, Eq, PartialEq, Debug, Ord, PartialOrd)]
pub struct ChannelId(pub String);

/// An identifier for a Message
#[derive(Clone, Hash, Eq, PartialEq, Debug, Ord, PartialOrd)]
pub struct MessageId(pub String);

/// An identifier for a Role
#[derive(Clone, Hash, Eq, PartialEq, Debug, Ord, PartialOrd)]
pub struct RoleId(pub String);

macro_rules! req {
	($opt:expr) => {
		try!($opt.ok_or(Error::Other(concat!(file!(), ":", line!(), ": ", stringify!($opt)))))
	}
}

macro_rules! warn_json {
	(@ $name:expr, $json:ident, $value:expr) => {
		(Ok($value), warn_field($name, $json)).0
	};
	($json:ident, $ty:ident $(::$ext:ident)* ( $($value:expr),*$(,)* ) ) => {
		(Ok($ty$(::$ext)* ( $($value),* )), warn_field(stringify!($ty$(::$ext)*), $json)).0
	};
	($json:ident, $ty:ident $(::$ext:ident)* { $($name:ident: $value:expr),*$(,)* } ) => {
		(Ok($ty$(::$ext)* { $($name: $value),* }), warn_field(stringify!($ty$(::$ext)*), $json)).0
	};
}

//=================
// Rest model

/// The type of a channel
#[derive(Copy, Clone, Hash, Eq, PartialEq, Debug)]
pub enum ChannelType {
	/// A text channel, through which `Message`s are transmitted
	Text,
	/// A voice channel
	Voice,
}

impl ChannelType {
	/// Attempt to parse a ChannelType from a name
	pub fn from_name(name: &str) -> Option<ChannelType> {
		if name == "text" {
			Some(ChannelType::Text)
		} else if name == "voice" {
			Some(ChannelType::Voice)
		} else {
			None
		}
	}

	/// Get the name of this ChannelType
	pub fn name(&self) -> &'static str {
		match *self {
			ChannelType::Text => "text",
			ChannelType::Voice => "voice",
		}
	}
}

/// Static information about a server
#[derive(Debug, Clone)]
pub struct Server {
	pub id: ServerId,
	pub name: String,
	pub joined_at: String, // Timestamp
	pub afk_timeout: u64,
	pub afk_channel_id: Option<ChannelId>,
	pub icon: Option<String>,
	pub roles: Vec<Role>,
	pub region: String,
	pub embed_enabled: bool,
	pub embed_channel_id: Option<ChannelId>,
	pub owner_id: UserId,
}

impl Server {
	pub fn decode(value: Value) -> Result<Server> {
		let mut value = try!(into_map(value));
		warn_json!(value, Server {
			id: try!(remove(&mut value, "id").and_then(into_string).map(ServerId)),
			name: try!(remove(&mut value, "name").and_then(into_string)),
			icon: remove(&mut value, "icon").and_then(into_string).ok(),
			joined_at: try!(remove(&mut value, "joined_at").and_then(into_string)),
			afk_timeout: req!(try!(remove(&mut value, "afk_timeout")).as_u64()),
			afk_channel_id: remove(&mut value, "afk_channel_id").and_then(into_string).map(ChannelId).ok(),
			embed_enabled: req!(try!(remove(&mut value, "embed_enabled")).as_boolean()),
			embed_channel_id: remove(&mut value, "embed_channel_id").and_then(into_string).map(ChannelId).ok(),
			owner_id: try!(remove(&mut value, "owner_id").and_then(into_string).map(UserId)),
			region: try!(remove(&mut value, "region").and_then(into_string)),
			roles: try!(decode_array(try!(remove(&mut value, "roles")), Role::decode)),
		})
	}
}

/// Information about a role
#[derive(Debug, Clone)]
pub struct Role {
	pub id: RoleId,
	pub name: String,
	/// Color in `0xRRGGBB` form
	pub color: u64,
	pub hoist: bool,
	pub managed: bool,
	pub position: i64,
	pub permissions: Permissions, // TODO: bitflags?
}

impl Role {
	pub fn decode(value: Value) -> Result<Role> {
		let mut value = try!(into_map(value));
		warn_json!(value, Role {
			id: try!(remove(&mut value, "id").and_then(into_string).map(RoleId)),
			name: try!(remove(&mut value, "name").and_then(into_string)),
			permissions: try!(remove(&mut value, "permissions").and_then(Permissions::decode)),
			color: req!(try!(remove(&mut value, "color")).as_u64()),
			hoist: req!(try!(remove(&mut value, "hoist")).as_boolean()),
			managed: req!(try!(remove(&mut value, "managed")).as_boolean()),
			position: req!(try!(remove(&mut value, "position")).as_i64()),
		})
	}
}

/// Broadly-applicable user information
#[derive(Debug, Clone)]
pub struct User {
	pub id: UserId,
	pub name: String,
	pub discriminator: String,
	pub avatar: Option<String>,
}

impl User {
	pub fn decode(value: Value) -> Result<User> {
		let mut value = try!(into_map(value));
		warn_json!(value, User {
			id: try!(remove(&mut value, "id").and_then(into_string).map(UserId)),
			name: try!(remove(&mut value, "username").and_then(into_string)),
			discriminator: try!(remove(&mut value, "discriminator").and_then(decode_discriminator)),
			avatar: remove(&mut value, "avatar").and_then(into_string).ok()
		})
	}
}

/// Information about a member of a server
#[derive(Debug, Clone)]
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
		warn_json!(value, Member {
			user: try!(remove(&mut value, "user").and_then(User::decode)),
			roles: try!(decode_array(try!(remove(&mut value, "roles")), |v| into_string(v).map(RoleId))),
			joined_at: try!(remove(&mut value, "joined_at").and_then(into_string)),
			mute: req!(try!(remove(&mut value, "mute")).as_boolean()),
			deaf: req!(try!(remove(&mut value, "deaf")).as_boolean()),
		})
	}
}

/// A private or public channel
#[derive(Debug, Clone)]
pub enum Channel {
	/// Text channel to another user
	Private(PrivateChannel),
	/// Voice or text channel within a server
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

/// Private text channel to another user
#[derive(Debug, Clone)]
pub struct PrivateChannel {
	pub id: ChannelId,
	pub recipient: User,
	pub last_message_id: Option<MessageId>,
}

impl PrivateChannel {
	pub fn decode(value: Value) -> Result<PrivateChannel> {
		let mut value = try!(into_map(value));
		value.remove("is_private"); // discard is_private
		warn_json!(value, PrivateChannel {
			id: try!(remove(&mut value, "id").and_then(into_string).map(ChannelId)),
			recipient: try!(remove(&mut value, "recipient").and_then(User::decode)),
			last_message_id: remove(&mut value, "last_message_id").and_then(into_string).map(MessageId).ok(),
		})
	}
}

/// Public voice or text channel within a server
#[derive(Debug, Clone)]
pub struct PublicChannel {
	pub id: ChannelId,
	pub name: String,
	pub server_id: ServerId,
	pub kind: ChannelType,
	pub permission_overwrites: Vec<PermissionOverwrite>,
	pub topic: Option<String>,
	pub position: u64,
	pub last_message_id: Option<MessageId>,
}

impl PublicChannel {
	pub fn decode(value: Value) -> Result<PublicChannel> {
		let mut value = try!(into_map(value));
		value.remove("is_private"); // discard is_private
		let id = try!(remove(&mut value, "guild_id").and_then(into_string).map(ServerId));
		PublicChannel::decode_server(Value::Object(value), id)
	}

	pub fn decode_server(value: Value, server_id: ServerId) -> Result<PublicChannel> {
		let mut value = try!(into_map(value));
		warn_json!(value, PublicChannel {
			id: try!(remove(&mut value, "id").and_then(into_string).map(ChannelId)),
			name: try!(remove(&mut value, "name").and_then(into_string)),
			server_id: server_id,
			topic: remove(&mut value, "topic").and_then(into_string).ok(),
			position: req!(try!(remove(&mut value, "position")).as_u64()),
			kind: try!(remove(&mut value, "type").and_then(into_string).and_then(|s| ChannelType::from_name(&s).ok_or(Error::Other("channel type")))),
			last_message_id: remove(&mut value, "last_message_id").and_then(into_string).map(MessageId).ok(),
			permission_overwrites: try!(decode_array(try!(remove(&mut value, "permission_overwrites")), PermissionOverwrite::decode)),
		})
	}
}

/// A channel-specific permission overwrite for a role or member
#[derive(Debug, Clone)]
pub enum PermissionOverwrite {
	Role { id: RoleId, allow: Permissions, deny: Permissions },
	Member { id: UserId, allow: Permissions, deny: Permissions },
}

impl PermissionOverwrite {
	pub fn decode(value: Value) -> Result<PermissionOverwrite> {
		let mut value = try!(into_map(value));
		let kind = try!(remove(&mut value, "type").and_then(into_string));
		let id = try!(remove(&mut value, "id").and_then(into_string));
		let allow = try!(remove(&mut value, "allow").and_then(Permissions::decode));
		let deny = try!(remove(&mut value, "deny").and_then(Permissions::decode));
		if kind == "role" {
			warn_json!(value, PermissionOverwrite::Role { id: RoleId(id), allow: allow, deny: deny })
		} else if kind == "member" {
			warn_json!(value, PermissionOverwrite::Member { id: UserId(id), allow: allow, deny: deny })
		} else {
			Err(Error::Decode(r#"PermissionOverwrite type ("role" or "member")"#, Value::String(kind)))
		}
	}
}

pub mod permissions {
	use ::{Error, Result};
	use serde_json::Value;

	bitflags! {
		/// Set of permissions assignable to a Role or PermissionOverwrite
		flags Permissions: u64 {
			const CREATE_INVITE = 1 << 0,
			const KICK_MEMBERS = 1 << 1,
			const BAN_MEMBERS = 1 << 2,
			/// Implies all permissions
			const MANAGE_ROLES = 1 << 3,
			/// Create channels or edit existing ones
			const MANAGE_CHANNELS = 1 << 4,
			/// Change the server's name or move regions
			const MANAGE_SERVER = 1 << 5,

			const READ_MESSAGES = 1 << 10,
			const SEND_MESSAGES = 1 << 11,
			/// Send text-to-speech messages to those focused on the channel
			const SEND_TTS_MESSAGES = 1 << 12,
			/// Delete or edit messages by other users
			const MANAGE_MESSAGES = 1 << 13,
			const EMBED_LINKS = 1 << 14,
			const ATTACH_FILES = 1 << 15,
			const READ_HISTORY = 1 << 16,
			/// Trigger a push notification for an entire channel with "@everyone"
			const MENTION_EVERYONE = 1 << 17,

			const VOICE_CONNECT = 1 << 20,
			const VOICE_SPEAK = 1 << 21,
			const VOICE_MUTE_MEMBERS = 1 << 22,
			const VOICE_DEAFEN_MEMBERS = 1 << 23,
			/// Move users out of this channel into another
			const VOICE_MOVE_MEMBERS = 1 << 24,
			/// When denied, members must use push-to-talk
			const VOICE_USE_VAD = 1 << 25,
		}
	}

	impl Permissions {
		pub fn decode(value: Value) -> Result<Permissions> {
			Ok(Self::from_bits_truncate(req!(value.as_u64())))
		}
	}
}

/// File upload attached to a message
#[derive(Debug, Clone)]
pub struct Attachment {
	pub id: String,
	/// Short filename for the attachment
	pub filename: String,
	/// Shorter URL with message and attachment id
	pub url: String,
	/// Longer URL with large hash
	pub proxy_url: String,
	/// Size of the file in bytes
	pub size: u64,
	/// Dimensions if the file is an image
	pub dimensions: Option<(u64, u64)>,
}

impl Attachment {
	pub fn decode(value: Value) -> Result<Attachment> {
		let mut value = try!(into_map(value));
		let width = remove(&mut value, "width").ok().and_then(|x| x.as_u64());
		let height = remove(&mut value, "height").ok().and_then(|x| x.as_u64());
		warn_json!(value, Attachment {
			id: try!(remove(&mut value, "id").and_then(into_string)),
			filename: try!(remove(&mut value, "filename").and_then(into_string)),
			url: try!(remove(&mut value, "url").and_then(into_string)),
			proxy_url: try!(remove(&mut value, "proxy_url").and_then(into_string)),
			size: req!(try!(remove(&mut value, "size")).as_u64()),
			dimensions: width.and_then(|w| height.map(|h| (w, h))),
		})
	}
}

/// Message transmitted over a text channel
#[derive(Debug, Clone)]
pub struct Message {
	pub id: MessageId,
	pub channel_id: ChannelId,
	pub content: String,
	pub nonce: Option<String>,
	pub tts: bool,
	pub timestamp: String,
	pub edited_timestamp: Option<String>,

	pub author: User,
	pub mention_everyone: bool,
	pub mentions: Vec<User>,

	pub attachments: Vec<Attachment>,
	/// Follows OEmbed standard
	pub embeds: Vec<Value>,
}

impl Message {
	pub fn decode(value: Value) -> Result<Message> {
		let mut value = try!(into_map(value));
		warn_json!(value, Message {
			id: try!(remove(&mut value, "id").and_then(into_string).map(MessageId)),
			channel_id: try!(remove(&mut value, "channel_id").and_then(into_string).map(ChannelId)),
			nonce: remove(&mut value, "nonce").and_then(into_string).ok(),
			content: try!(remove(&mut value, "content").and_then(into_string)),
			tts: req!(try!(remove(&mut value, "tts")).as_boolean()),
			timestamp: try!(remove(&mut value, "timestamp").and_then(into_string)),
			edited_timestamp: remove(&mut value, "edited_timestamp").and_then(into_string).ok(),
			mention_everyone: req!(try!(remove(&mut value, "mention_everyone")).as_boolean()),
			mentions: try!(decode_array(try!(remove(&mut value, "mentions")), User::decode)),
			author: try!(remove(&mut value, "author").and_then(User::decode)),
			attachments: try!(decode_array(try!(remove(&mut value, "attachments")), Attachment::decode)),
			embeds: try!(decode_array(try!(remove(&mut value, "embeds")), Ok)),
		})
	}
}

//=================
// Event model

/// Summary of messages since last login
#[derive(Debug, Clone)]
pub struct ReadState {
	/// Id of the relevant channel
	pub id: ChannelId,
	/// Last seen message in this channel
	pub last_message_id: Option<MessageId>,
	/// Mentions since that message in this channel
	pub mention_count: u64,
}

impl ReadState {
	pub fn decode(value: Value) -> Result<ReadState> {
		let mut value = try!(into_map(value));
		warn_json!(value, ReadState {
			id: try!(remove(&mut value, "id").and_then(into_string).map(ChannelId)),
			last_message_id: remove(&mut value, "last_message_id").and_then(into_string).map(MessageId).ok(),
			mention_count: req!(try!(remove(&mut value, "mention_count")).as_u64()),
		})
	}
}

/// A members's online status
#[derive(Debug, Clone)]
pub struct Presence {
	pub user_id: UserId,
	pub user: Option<User>,
	pub status: String, // enum?
	pub game_id: Option<u64>,
}

impl Presence {
	pub fn decode(value: Value) -> Result<Presence> {
		let mut value = try!(into_map(value));
		let mut user_map = try!(remove(&mut value, "user").and_then(into_map));

		let (user_id, user) = if user_map.len() > 1 {
			let user = try!(User::decode(Value::Object(user_map)));
			(user.id.clone(), Some(user))
		} else {
			(try!(remove(&mut user_map, "id").and_then(into_string).map(UserId)), None)
		};

		warn_json!(value, Presence {
			user_id: user_id,
			user: user,
			status: try!(remove(&mut value, "status").and_then(into_string)),
			game_id: remove(&mut value, "game_id").ok().and_then(|x| x.as_u64()),
		})
	}
}

/// A member's state within a voice channel
#[derive(Debug, Clone)]
pub struct VoiceState {
	pub user_id: UserId,
	pub channel_id: Option<ChannelId>,
	pub session_id: String,
	pub token: Option<String>,
	pub suppress: bool,
	pub self_mute: bool,
	pub self_deaf: bool,
	pub mute: bool,
	pub deaf: bool,
}

impl VoiceState {
	pub fn decode(value: Value) -> Result<VoiceState> {
		let mut value = try!(into_map(value));
		warn_json!(value, VoiceState {
			user_id: try!(remove(&mut value, "user_id").and_then(into_string).map(UserId)),
			channel_id: remove(&mut value, "channel_id").and_then(into_string).map(ChannelId).ok(),
			session_id: try!(remove(&mut value, "session_id").and_then(into_string)),
			token: remove(&mut value, "token").and_then(into_string).ok(),
			suppress: req!(req!(value.remove("suppress")).as_boolean()),
			self_mute: req!(req!(value.remove("self_mute")).as_boolean()),
			self_deaf: req!(req!(value.remove("self_deaf")).as_boolean()),
			mute: req!(req!(value.remove("mute")).as_boolean()),
			deaf: req!(req!(value.remove("deaf")).as_boolean()),
		})
	}
}

/// Live server information
#[derive(Debug, Clone)]
pub struct LiveServer {
	pub id: ServerId,
	pub name: String,
	pub owner_id: UserId,
	pub voice_states: Vec<VoiceState>,
	pub roles: Vec<Role>,
	pub region: String,
	pub presences: Vec<Presence>,
	pub members: Vec<Member>,
	pub joined_at: String,
	pub icon: Option<String>,
	pub large: bool,
	pub channels: Vec<PublicChannel>,
	pub afk_timeout: u64,
	pub afk_channel_id: Option<ChannelId>,
}

impl LiveServer {
	pub fn decode(value: Value) -> Result<LiveServer> {
		let mut value = try!(into_map(value));
		let id = try!(remove(&mut value, "id").and_then(into_string).map(ServerId));
		warn_json!(value, LiveServer {
			name: try!(remove(&mut value, "name").and_then(into_string)),
			owner_id: try!(remove(&mut value, "owner_id").and_then(into_string).map(UserId)),
			voice_states: try!(decode_array(try!(remove(&mut value, "voice_states")), VoiceState::decode)),
			roles: try!(decode_array(try!(remove(&mut value, "roles")), Role::decode)),
			region: try!(remove(&mut value, "region").and_then(into_string)),
			presences: try!(decode_array(try!(remove(&mut value, "presences")), Presence::decode)),
			members: try!(decode_array(try!(remove(&mut value, "members")), Member::decode)),
			joined_at: try!(remove(&mut value, "joined_at").and_then(into_string)),
			icon: remove(&mut value, "icon").and_then(into_string).ok(),
			large: req!(try!(remove(&mut value, "large")).as_boolean()),
			afk_timeout: req!(try!(remove(&mut value, "afk_timeout")).as_u64()),
			afk_channel_id: remove(&mut value, "afk_channel_id").and_then(into_string).map(ChannelId).ok(),
			channels: try!(decode_array(try!(remove(&mut value, "channels")), |v| PublicChannel::decode_server(v, id.clone()))),
			id: id,
		})
	}
}

/// Information about the logged-in user
#[derive(Debug, Clone)]
pub struct CurrentUser {
	pub id: UserId,
	pub username: String,
	pub discriminator: String,
	pub email: String,
	pub verified: bool,
	pub avatar: Option<String>,
}

impl CurrentUser {
	pub fn decode(value: Value) -> Result<CurrentUser> {
		let mut value = try!(into_map(value));
		warn_json!(value, CurrentUser {
			id: try!(remove(&mut value, "id").and_then(into_string).map(UserId)),
			username: try!(remove(&mut value, "username").and_then(into_string)),
			discriminator: try!(remove(&mut value, "discriminator").and_then(into_string)),
			email: try!(remove(&mut value, "email").and_then(into_string)),
			avatar: remove(&mut value, "avatar").and_then(into_string).ok(),
			verified: req!(try!(remove(&mut value, "verified")).as_boolean()),
		})
	}
}

/// The "Ready" event, containing initial state
#[derive(Debug, Clone)]
pub struct ReadyEvent {
	pub version: u64,
	pub user: CurrentUser,
	pub session_id: String,
	pub heartbeat_interval: u64,
	pub read_state: Vec<ReadState>,
	pub private_channels: Vec<PrivateChannel>,
	pub servers: Vec<LiveServer>,
}

/// Event received over a websocket connection
#[derive(Debug, Clone)]
pub enum Event {
	/// The first event in a connection, containing the initial state
	Ready(ReadyEvent),
	/// Update to the logged-in user's information
	UserUpdate(CurrentUser),
	/// A member's voice state has changed
	VoiceStateUpdate(ServerId, VoiceState),
	/// A user is typing; considered to last 5 seconds
	TypingStart {
		channel_id: ChannelId,
		user_id: UserId,
		timestamp: u64,
	},
	/// A member's presence state has changed
	PresenceUpdate {
		server_id: ServerId,
		presence: Presence,
		roles: Vec<RoleId>,
	},

	MessageCreate(Message),
	/// A message has been edited, either by the user or the system
	MessageUpdate {
		id: MessageId,
		channel_id: ChannelId,
		/* TODO: the remaining fields
		content: Option<String>,
		tts: Option<bool>,
		timestamp: Option<String>,
		edited_timestamp: Option<String>,
		author: Option<User>,
		embeds: Option<Vec<Value>>,
		mention_everyone: Option<bool>,
		mentions: Option<Vec<User>>,
		attachments: Option<Vec<Attachment>>,
		embeds: Option<Vec<Value>>,*/
	},
	/// Another logged-in device acknowledged this message
	MessageAck {
		message_id: MessageId,
		channel_id: ChannelId,
	},
	MessageDelete {
		message_id: MessageId,
		channel_id: ChannelId,
	},

	ServerCreate(LiveServer),
	ServerUpdate(Server),
	ServerDelete(Server),

	ServerMemberAdd {
		server_id: ServerId,
		joined_at: String, // timestamp
		roles: Vec<RoleId>,
		user: User,
	},
	ServerMemberUpdate {
		server_id: ServerId,
		roles: Vec<RoleId>,
		user: User,
	},
	ServerMemberRemove(ServerId, User),

	ServerRoleCreate(ServerId, Role),
	ServerRoleUpdate(ServerId, Role),
	ServerRoleDelete(ServerId, RoleId),

	ChannelCreate(Channel),
	ChannelUpdate(Channel),
	ChannelDelete(Channel),

	/// An event type not covered by the above
	Unknown(String, BTreeMap<String, Value>),
	/// A websocket "close" frame with the given status
	Closed(u16),
}

impl Event {
	pub fn decode(value: Value) -> Result<Event> {
		let mut value = try!(into_map(value));

		let op = req!(req!(value.remove("op")).as_u64());
		if op != 0 {
			value.insert("op".into(), Value::U64(op));
			return Err(Error::Decode("Unknown opcode", Value::Object(value)))
		}

		let kind = try!(remove(&mut value, "t").and_then(into_string));
		let mut value = try!(remove(&mut value, "d").and_then(into_map));
		if kind == "READY" {
			warn_json!(value, Event::Ready(ReadyEvent {
				version: req!(try!(remove(&mut value, "v")).as_u64()),
				user: try!(remove(&mut value, "user").and_then(CurrentUser::decode)),
				session_id: try!(remove(&mut value, "session_id").and_then(into_string)),
				heartbeat_interval: req!(try!(remove(&mut value, "heartbeat_interval")).as_u64()),
				read_state: try!(decode_array(try!(remove(&mut value, "read_state")), ReadState::decode)),
				private_channels: try!(decode_array(try!(remove(&mut value, "private_channels")), PrivateChannel::decode)),
				servers: try!(decode_array(try!(remove(&mut value, "guilds")), LiveServer::decode)),
			}))
		} else if kind == "USER_UPDATE" {
			CurrentUser::decode(Value::Object(value)).map(Event::UserUpdate)
		} else if kind == "VOICE_STATE_UPDATE" {
			let server_id = try!(remove(&mut value, "guild_id").and_then(into_string).map(ServerId));
			Ok(Event::VoiceStateUpdate(server_id, try!(VoiceState::decode(Value::Object(value)))))
		} else if kind == "TYPING_START" {
			warn_json!(value, Event::TypingStart {
				channel_id: try!(remove(&mut value, "channel_id").and_then(into_string).map(ChannelId)),
				user_id: try!(remove(&mut value, "user_id").and_then(into_string).map(UserId)),
				timestamp: req!(try!(remove(&mut value, "timestamp")).as_u64()),
			})
		} else if kind == "PRESENCE_UPDATE" {
			let server_id = try!(remove(&mut value, "guild_id").and_then(into_string).map(ServerId));
			Ok(Event::PresenceUpdate {
				server_id: server_id,
				roles: try!(decode_array(try!(remove(&mut value, "roles")), |x| into_string(x).map(RoleId))),
				presence: try!(Presence::decode(Value::Object(value))),
			})
		} else if kind == "MESSAGE_CREATE" {
			Message::decode(Value::Object(value)).map(Event::MessageCreate)
		} else if kind == "MESSAGE_UPDATE" {
			warn_json!(value, Event::MessageUpdate {
				id: try!(remove(&mut value, "id").and_then(into_string).map(MessageId)),
				channel_id: try!(remove(&mut value, "channel_id").and_then(into_string).map(ChannelId)),
				// TODO: more fields
			})
		} else if kind == "MESSAGE_ACK" {
			warn_json!(value, Event::MessageAck {
				message_id: try!(remove(&mut value, "message_id").and_then(into_string).map(MessageId)),
				channel_id: try!(remove(&mut value, "channel_id").and_then(into_string).map(ChannelId)),
			})
		} else if kind == "MESSAGE_DELETE" {
			warn_json!(value, Event::MessageDelete {
				message_id: try!(remove(&mut value, "id").and_then(into_string).map(MessageId)),
				channel_id: try!(remove(&mut value, "channel_id").and_then(into_string).map(ChannelId)),
			})
		} else if kind == "GUILD_CREATE" {
			LiveServer::decode(Value::Object(value)).map(Event::ServerCreate)
		} else if kind == "GUILD_UPDATE" {
			Server::decode(Value::Object(value)).map(Event::ServerUpdate)
		} else if kind == "GUILD_DELETE" {
			Server::decode(Value::Object(value)).map(Event::ServerDelete)
		} else if kind == "GUILD_MEMBER_ADD" {
			warn_json!(value, Event::ServerMemberAdd {
				server_id: try!(remove(&mut value, "guild_id").and_then(into_string).map(ServerId)),
				joined_at: try!(remove(&mut value, "joined_at").and_then(into_string)),
				roles: try!(decode_array(try!(remove(&mut value, "roles")), |x| into_string(x).map(RoleId))),
				user: try!(remove(&mut value, "user").and_then(User::decode)),
			})
		} else if kind == "GUILD_MEMBER_UPDATE" {
			warn_json!(value, Event::ServerMemberUpdate {
				server_id: try!(remove(&mut value, "guild_id").and_then(into_string).map(ServerId)),
				roles: try!(decode_array(try!(remove(&mut value, "roles")), |x| into_string(x).map(RoleId))),
				user: try!(remove(&mut value, "user").and_then(User::decode)),
			})
		} else if kind == "GUILD_MEMBER_REMOVE" {
			warn_json!(value, Event::ServerMemberRemove(
				try!(remove(&mut value, "guild_id").and_then(into_string).map(ServerId)),
				try!(remove(&mut value, "user").and_then(User::decode)),
			))
		} else if kind == "GUILD_ROLE_CREATE" {
			warn_json!(value, Event::ServerRoleCreate(
				try!(remove(&mut value, "guild_id").and_then(into_string).map(ServerId)),
				try!(remove(&mut value, "role").and_then(Role::decode)),
			))
		} else if kind == "GUILD_ROLE_UPDATE" {
			warn_json!(value, Event::ServerRoleUpdate(
				try!(remove(&mut value, "guild_id").and_then(into_string).map(ServerId)),
				try!(remove(&mut value, "role").and_then(Role::decode)),
			))
		} else if kind == "GUILD_ROLE_DELETE" {
			warn_json!(value, Event::ServerRoleDelete(
				try!(remove(&mut value, "guild_id").and_then(into_string).map(ServerId)),
				try!(remove(&mut value, "role_id").and_then(into_string).map(RoleId)),
			))
		} else if kind == "CHANNEL_CREATE" {
			Channel::decode(Value::Object(value)).map(Event::ChannelCreate)
		} else if kind == "CHANNEL_UPDATE" {
			Channel::decode(Value::Object(value)).map(Event::ChannelUpdate)
		} else if kind == "CHANNEL_DELETE" {
			Channel::decode(Value::Object(value)).map(Event::ChannelDelete)
		} else {
			Ok(Event::Unknown(kind, value))
		}
	}
}

//=================
// Decode helpers

fn remove(map: &mut BTreeMap<String, Value>, key: &'static str) -> Result<Value> {
	map.remove(key).ok_or(Error::Decode(key, Value::Null))
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

fn warn_field(name: &str, map: BTreeMap<String, Value>) {
	if map.len() != 0 {
		println!("[Warning] Unhandled keys: {} has {:?}", name, Value::Object(map))
	}
}
