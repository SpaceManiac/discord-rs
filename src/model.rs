//! Struct and enum definitions of values in the Discord model.
#![allow(missing_docs)]

// TODO: When reading optional keys, type errors are silently treated as if the
// key was absent. Either decoding should fail or a warning should be printed.

use super::{Error, Result};
use serde_json::Value;
use std::collections::BTreeMap;

pub use self::permissions::Permissions;

macro_rules! req {
	($opt:expr) => {
		try!($opt.ok_or(Error::Decode(concat!("Type mismatch in model:", line!(), ": ", stringify!($opt)), Value::Null)))
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
// Discord identifier types

fn decode_id(value: Value) -> Result<u64> {
	let value = try!(into_string(value));
	match value.parse::<u64>() {
		Ok(num) => Ok(num),
		Err(_) => Err(Error::Decode("Expected numeric string", Value::String(value)))
	}
}

macro_rules! id {
	($(#[$attr:meta] $name:ident;)*) => {
		$(
			#[$attr]
			#[derive(Copy, Clone, Hash, Eq, PartialEq, Debug, Ord, PartialOrd)]
			pub struct $name(pub u64);

			impl $name {
				#[inline]
				fn decode(value: Value) -> Result<Self> {
					decode_id(value).map($name)
				}

				/// Get the creation date of the object referred to by this ID.
				///
				/// Discord generates identifiers using a scheme based on [Twitter Snowflake]
				/// (https://github.com/twitter/snowflake/tree/b3f6a3c6ca8e1b6847baa6ff42bf72201e2c2231#snowflake).
				pub fn creation_date(&self) -> ::time::Timespec {
					::time::Timespec::new((1420070400 + (self.0 >> 22) / 1000) as i64, 0)
				}
			}
		)*
	}
}

id! {
	/// An identifier for a User
	UserId;
	/// An identifier for a Server
	ServerId;
	/// An identifier for a Channel
	ChannelId;
	/// An identifier for a Message
	MessageId;
	/// An identifier for a Role
	RoleId;
	/// An identifier for an Emoji
	EmojiId;
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
	pub fn from_str(name: &str) -> Option<ChannelType> {
		match name {
			"text" => Some(ChannelType::Text),
			"voice" => Some(ChannelType::Voice),
			_ => None,
		}
	}

	fn from_str_err(name: String) -> Result<ChannelType> {
		ChannelType::from_str(&name).ok_or(Error::Decode("Expected valid ChannelType", Value::String(name)))
	}

	/// Get the name of this ChannelType
	pub fn name(&self) -> &'static str {
		match *self {
			ChannelType::Text => "text",
			ChannelType::Voice => "voice",
		}
	}
}

/// The basic information about a server only
#[derive(Debug, Clone)]
pub struct ServerInfo {
	pub id: ServerId,
	pub name: String,
	pub icon: Option<String>,
	pub owner: bool,
}

impl ServerInfo {
	pub fn decode(value: Value) -> Result<Self> {
		let mut value = try!(into_map(value));
		warn_json!(value, ServerInfo {
			id: try!(remove(&mut value, "id").and_then(ServerId::decode)),
			name: try!(remove(&mut value, "name").and_then(into_string)),
			icon: remove(&mut value, "icon").and_then(into_string).ok(),
			owner: req!(try!(remove(&mut value, "owner")).as_boolean()),
		})
	}
}

/// Static information about a server
#[derive(Debug, Clone)]
pub struct Server {
	pub id: ServerId,
	pub name: String,
	pub afk_timeout: u64,
	pub afk_channel_id: Option<ChannelId>,
	pub icon: Option<String>,
	pub roles: Vec<Role>,
	pub region: String,
	pub embed_enabled: bool,
	pub embed_channel_id: Option<ChannelId>,
	pub owner_id: UserId,
	pub verification_level: VerificationLevel,
}

impl Server {
	pub fn decode(value: Value) -> Result<Server> {
		let mut value = try!(into_map(value));
		warn_json!(value, Server {
			id: try!(remove(&mut value, "id").and_then(ServerId::decode)),
			name: try!(remove(&mut value, "name").and_then(into_string)),
			icon: remove(&mut value, "icon").and_then(into_string).ok(),
			afk_timeout: req!(try!(remove(&mut value, "afk_timeout")).as_u64()),
			afk_channel_id: remove(&mut value, "afk_channel_id").and_then(ChannelId::decode).ok(),
			embed_enabled: req!(try!(remove(&mut value, "embed_enabled")).as_boolean()),
			embed_channel_id: remove(&mut value, "embed_channel_id").and_then(ChannelId::decode).ok(),
			owner_id: try!(remove(&mut value, "owner_id").and_then(UserId::decode)),
			region: try!(remove(&mut value, "region").and_then(into_string)),
			roles: try!(decode_array(try!(remove(&mut value, "roles")), Role::decode)),
			verification_level: try!(remove(&mut value, "verification_level").and_then(VerificationLevel::decode)),
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
	pub permissions: Permissions,
}

impl Role {
	pub fn decode(value: Value) -> Result<Role> {
		let mut value = try!(into_map(value));
		warn_json!(value, Role {
			id: try!(remove(&mut value, "id").and_then(RoleId::decode)),
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
	pub bot: bool,
}

impl User {
	pub fn decode(value: Value) -> Result<User> {
		let mut value = try!(into_map(value));
		warn_json!(value, User {
			id: try!(remove(&mut value, "id").and_then(UserId::decode)),
			name: try!(remove(&mut value, "username").and_then(into_string)),
			discriminator: try!(remove(&mut value, "discriminator").and_then(decode_discriminator)),
			avatar: remove(&mut value, "avatar").and_then(into_string).ok(),
			bot: remove(&mut value, "bot").ok().and_then(|x| x.as_boolean()).unwrap_or(false),
		})
	}

	#[doc(hidden)]
	pub fn decode_ban(value: Value) -> Result<User> {
		let mut value = try!(into_map(value));
		warn_json!(@"Ban", value, try!(remove(&mut value, "user").and_then(User::decode)))
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
			roles: try!(decode_array(try!(remove(&mut value, "roles")), RoleId::decode)),
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
		if req!(try!(remove(&mut value, "is_private")).as_boolean()) {
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
			id: try!(remove(&mut value, "id").and_then(ChannelId::decode)),
			recipient: try!(remove(&mut value, "recipient").and_then(User::decode)),
			last_message_id: remove(&mut value, "last_message_id").and_then(MessageId::decode).ok(),
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
	pub position: i64,
	pub last_message_id: Option<MessageId>,
	pub bitrate: Option<u64>,
}

impl PublicChannel {
	pub fn decode(value: Value) -> Result<PublicChannel> {
		let mut value = try!(into_map(value));
		value.remove("is_private"); // discard is_private
		let id = try!(remove(&mut value, "guild_id").and_then(ServerId::decode));
		PublicChannel::decode_server(Value::Object(value), id)
	}

	pub fn decode_server(value: Value, server_id: ServerId) -> Result<PublicChannel> {
		let mut value = try!(into_map(value));
		warn_json!(value, PublicChannel {
			id: try!(remove(&mut value, "id").and_then(ChannelId::decode)),
			name: try!(remove(&mut value, "name").and_then(into_string)),
			server_id: server_id,
			topic: remove(&mut value, "topic").and_then(into_string).ok(),
			position: req!(try!(remove(&mut value, "position")).as_i64()),
			kind: try!(remove(&mut value, "type").and_then(into_string).and_then(ChannelType::from_str_err)),
			last_message_id: remove(&mut value, "last_message_id").and_then(MessageId::decode).ok(),
			permission_overwrites: try!(decode_array(try!(remove(&mut value, "permission_overwrites")), PermissionOverwrite::decode)),
			bitrate: remove(&mut value, "bitrate").ok().and_then(|v| v.as_u64()),
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
		let id = try!(remove(&mut value, "id").and_then(decode_id));
		let allow = try!(remove(&mut value, "allow").and_then(Permissions::decode));
		let deny = try!(remove(&mut value, "deny").and_then(Permissions::decode));
		if kind == "role" {
			warn_json!(value, PermissionOverwrite::Role { id: RoleId(id), allow: allow, deny: deny })
		} else if kind == "member" {
			warn_json!(value, PermissionOverwrite::Member { id: UserId(id), allow: allow, deny: deny })
		} else {
			Err(Error::Decode("Expected valid PermissionOverwrite type", Value::String(kind)))
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
			id: try!(remove(&mut value, "id").and_then(MessageId::decode)),
			channel_id: try!(remove(&mut value, "channel_id").and_then(ChannelId::decode)),
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

/// Information about an invite
#[derive(Debug, Clone)]
pub struct Invite {
	pub code: String,
	pub xkcdpass: Option<String>,
	pub server_id: ServerId,
	pub server_name: String,
	pub channel_type: ChannelType,
	pub channel_id: ChannelId,
	pub channel_name: String,
}

impl Invite {
	pub fn decode(value: Value) -> Result<Invite> {
		let mut value = try!(into_map(value));

		let mut server = try!(remove(&mut value, "guild").and_then(into_map));
		let server_id = try!(remove(&mut server, "id").and_then(ServerId::decode));
		let server_name = try!(remove(&mut server, "name").and_then(into_string));
		warn_field("Invite/guild", server);

		let mut channel = try!(remove(&mut value, "channel").and_then(into_map));
		let channel_type = try!(remove(&mut channel, "type").and_then(into_string).and_then(ChannelType::from_str_err));
		let channel_id = try!(remove(&mut channel, "id").and_then(ChannelId::decode));
		let channel_name = try!(remove(&mut channel, "name").and_then(into_string));
		warn_field("Invite/channel", channel);

		warn_json!(value, Invite {
			code: try!(remove(&mut value, "code").and_then(into_string)),
			xkcdpass: remove(&mut value, "xkcdpass").and_then(into_string).ok(),
			server_id: server_id,
			server_name: server_name,
			channel_type: channel_type,
			channel_id: channel_id,
			channel_name: channel_name,
		})
	}
}

/// Information about an available voice region
#[derive(Debug, Clone)]
pub struct VoiceRegion {
	pub id: String,
	pub name: String,
	pub sample_hostname: String,
	pub sample_port: u16,
	pub optimal: bool,
	pub vip: bool,
}

impl VoiceRegion {
	pub fn decode(value: Value) -> Result<VoiceRegion> {
		let mut value = try!(into_map(value));
		warn_json!(value, VoiceRegion {
			id: try!(remove(&mut value, "id").and_then(into_string)),
			name: try!(remove(&mut value, "name").and_then(into_string)),
			sample_hostname: try!(remove(&mut value, "sample_hostname").and_then(into_string)),
			sample_port: req!(try!(remove(&mut value, "sample_port")).as_u64()) as u16,
			optimal: req!(try!(remove(&mut value, "optimal")).as_boolean()),
			vip: req!(try!(remove(&mut value, "vip")).as_boolean()),
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
			id: try!(remove(&mut value, "id").and_then(ChannelId::decode)),
			last_message_id: remove(&mut value, "last_message_id").and_then(MessageId::decode).ok(),
			mention_count: req!(try!(remove(&mut value, "mention_count")).as_u64()),
		})
	}
}

/// A user's online presence status
#[derive(Copy, Clone, Hash, Eq, PartialEq, Debug)]
pub enum OnlineStatus {
	Offline,
	Online,
	Idle,
}

impl OnlineStatus {
	pub fn from_str(name: &str) -> Option<OnlineStatus> {
		match name {
			"offline" => Some(OnlineStatus::Offline),
			"online" => Some(OnlineStatus::Online),
			"idle" => Some(OnlineStatus::Idle),
			_ => None,
		}
	}

	fn from_str_err(name: String) -> Result<OnlineStatus> {
		OnlineStatus::from_str(&name).ok_or(Error::Decode("Expected valid OnlineStatus", Value::String(name)))
	}
}

/// Information about a game being played
#[derive(Debug, Clone)]
pub struct Game {
	pub name: String,
}

impl Game {
	pub fn decode(value: Value) -> Result<Game> {
		let mut value = try!(into_map(value));
		warn_json!(value, Game {
			name: try!(remove(&mut value, "name").and_then(into_string)),
		})
	}
}

/// A members's online status
#[derive(Debug, Clone)]
pub struct Presence {
	pub user_id: UserId,
	pub status: OnlineStatus,
	pub game: Option<Game>,
}

impl Presence {
	pub fn decode(value: Value) -> Result<(Presence, Option<User>)> {
		let mut value = try!(into_map(value));
		let mut user_map = try!(remove(&mut value, "user").and_then(into_map));

		let (user_id, user) = if user_map.len() > 1 {
			let user = try!(User::decode(Value::Object(user_map)));
			(user.id.clone(), Some(user))
		} else {
			(try!(remove(&mut user_map, "id").and_then(UserId::decode)), None)
		};

		warn_json!(@"Presence", value, (Presence {
			user_id: user_id,
			status: try!(remove(&mut value, "status").and_then(into_string).and_then(OnlineStatus::from_str_err)),
			game: remove(&mut value, "game").and_then(Game::decode).ok(),
		}, user))
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
			user_id: try!(remove(&mut value, "user_id").and_then(UserId::decode)),
			channel_id: remove(&mut value, "channel_id").and_then(ChannelId::decode).ok(),
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

/// A condition that new users must satisfy before posting in text channels
#[derive(Copy, Clone, Hash, Eq, PartialEq, Debug)]
pub enum VerificationLevel {
	/// No verification is needed
	None,
	/// Must have a verified email on their Discord account
	Low,
	/// Must also be registered on Discord for longer than 5 minutes
	Medium,
	/// Must also be a member of this server for longer than 10 minutes
	High,
}

impl VerificationLevel {
	pub fn from_num(level: u64) -> Option<VerificationLevel> {
		match level {
			0 => Some(VerificationLevel::None),
			1 => Some(VerificationLevel::Low),
			2 => Some(VerificationLevel::Medium),
			3 => Some(VerificationLevel::High),
			_ => None,
		}
	}

	fn decode(value: Value) -> Result<VerificationLevel> {
		value.as_u64().and_then(VerificationLevel::from_num).ok_or(Error::Decode("Expected valid VerificationLevel", value))
	}
}

/// A parter custom emoji
#[derive(Debug, Clone)]
pub struct Emoji {
	pub id: EmojiId,
	pub name: String,
	pub managed: bool,
	pub require_colons: bool,
	pub roles: Vec<RoleId>,
}

impl Emoji {
	pub fn decode(value: Value) -> Result<Self> {
		let mut value = try!(into_map(value));
		warn_json!(value, Emoji {
			id: try!(remove(&mut value, "id").and_then(EmojiId::decode)),
			name: try!(remove(&mut value, "name").and_then(into_string)),
			managed: req!(try!(remove(&mut value, "managed")).as_boolean()),
			require_colons: req!(try!(remove(&mut value, "require_colons")).as_boolean()),
			roles: try!(remove(&mut value, "roles").and_then(|v| decode_array(v, RoleId::decode))),
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
	pub member_count: u64,
	pub members: Vec<Member>,
	pub joined_at: String,
	pub icon: Option<String>,
	pub large: bool,
	pub channels: Vec<PublicChannel>,
	pub afk_timeout: u64,
	pub afk_channel_id: Option<ChannelId>,
	pub verification_level: VerificationLevel,
	pub emojis: Vec<Emoji>,
	pub features: Vec<String>,
	pub splash: Option<String>,
}

impl LiveServer {
	pub fn decode(value: Value) -> Result<LiveServer> {
		let mut value = try!(into_map(value));
		let id = try!(remove(&mut value, "id").and_then(ServerId::decode));
		warn_json!(value, LiveServer {
			name: try!(remove(&mut value, "name").and_then(into_string)),
			owner_id: try!(remove(&mut value, "owner_id").and_then(UserId::decode)),
			voice_states: try!(decode_array(try!(remove(&mut value, "voice_states")), VoiceState::decode)),
			roles: try!(decode_array(try!(remove(&mut value, "roles")), Role::decode)),
			region: try!(remove(&mut value, "region").and_then(into_string)),
			// these presences don't contain a whole User, so discard that
			presences: try!(decode_array(try!(remove(&mut value, "presences")), |v| Presence::decode(v).map(|x| x.0))),
			member_count: req!(try!(remove(&mut value, "member_count")).as_u64()),
			members: try!(decode_array(try!(remove(&mut value, "members")), Member::decode)),
			joined_at: try!(remove(&mut value, "joined_at").and_then(into_string)),
			icon: remove(&mut value, "icon").and_then(into_string).ok(),
			large: req!(try!(remove(&mut value, "large")).as_boolean()),
			afk_timeout: req!(try!(remove(&mut value, "afk_timeout")).as_u64()),
			afk_channel_id: remove(&mut value, "afk_channel_id").and_then(ChannelId::decode).ok(),
			channels: try!(decode_array(try!(remove(&mut value, "channels")), |v| PublicChannel::decode_server(v, id.clone()))),
			verification_level: try!(remove(&mut value, "verification_level").and_then(VerificationLevel::decode)),
			emojis: try!(remove(&mut value, "emojis").and_then(|v| decode_array(v, Emoji::decode))),
			features: try!(remove(&mut value, "features").and_then(|v| decode_array(v, into_string))),
			splash: remove(&mut value, "splash").and_then(into_string).ok(),
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
	pub email: Option<String>,
	pub verified: bool,
	pub avatar: Option<String>,
}

impl CurrentUser {
	pub fn decode(value: Value) -> Result<CurrentUser> {
		let mut value = try!(into_map(value));
		warn_json!(value, CurrentUser {
			id: try!(remove(&mut value, "id").and_then(UserId::decode)),
			username: try!(remove(&mut value, "username").and_then(into_string)),
			discriminator: try!(remove(&mut value, "discriminator").and_then(into_string)),
			email: remove(&mut value, "email").and_then(into_string).ok(),
			avatar: remove(&mut value, "avatar").and_then(into_string).ok(),
			verified: req!(try!(remove(&mut value, "verified")).as_boolean()),
		})
	}
}

/// User settings usually used to influence client behavior
#[derive(Debug, Clone)]
pub struct UserSettings {
	pub enable_tts_command: bool,
	pub inline_attachment_media: bool,
	pub inline_embed_media: bool,
	pub locale: String,
	pub message_display_compact: bool,
	pub render_embeds: bool,
	pub show_current_game: bool,
	pub theme: String,
	pub convert_emoticons: bool,
}

impl UserSettings {
	pub fn decode(value: Value) -> Result<UserSettings> {
		let mut value = try!(into_map(value));
		warn_json!(value, UserSettings {
			enable_tts_command: req!(try!(remove(&mut value, "enable_tts_command")).as_boolean()),
			inline_attachment_media: req!(try!(remove(&mut value, "inline_attachment_media")).as_boolean()),
			inline_embed_media: req!(try!(remove(&mut value, "inline_embed_media")).as_boolean()),
			locale: try!(remove(&mut value, "locale").and_then(into_string)),
			message_display_compact: req!(try!(remove(&mut value, "message_display_compact")).as_boolean()),
			render_embeds: req!(try!(remove(&mut value, "render_embeds")).as_boolean()),
			show_current_game: req!(try!(remove(&mut value, "show_current_game")).as_boolean()),
			theme: try!(remove(&mut value, "theme").and_then(into_string)),
			convert_emoticons: req!(try!(remove(&mut value, "convert_emoticons")).as_boolean()),
		})
	}
}

/// A user's online presence status
#[derive(Copy, Clone, Hash, Eq, PartialEq, Debug)]
pub enum NotificationLevel {
	/// All messages trigger a notification
	All,
	/// Only @mentions trigger a notification
	Mentions,
	/// No messages, even @mentions, trigger a notification
	Nothing,
	/// Follow the parent's notification level
	Parent,
}

impl NotificationLevel {
	pub fn from_num(level: u64) -> Option<NotificationLevel> {
		match level {
			0 => Some(NotificationLevel::All),
			1 => Some(NotificationLevel::Mentions),
			2 => Some(NotificationLevel::Nothing),
			3 => Some(NotificationLevel::Parent),
			_ => None,
		}
	}

	fn decode(value: Value) -> Result<NotificationLevel> {
		value.as_u64().and_then(NotificationLevel::from_num).ok_or(Error::Decode("Expected valid NotificationLevel", value))
	}
}

/// A channel-specific notification settings override
#[derive(Debug, Clone)]
pub struct ChannelOverride {
	pub channel_id: ChannelId,
	pub message_notifications: NotificationLevel,
	pub muted: bool,
}

impl ChannelOverride {
	pub fn decode(value: Value) -> Result<ChannelOverride> {
		let mut value = try!(into_map(value));
		warn_json!(value, ChannelOverride {
			channel_id: try!(remove(&mut value, "channel_id").and_then(ChannelId::decode)),
			message_notifications: try!(remove(&mut value, "message_notifications").and_then(NotificationLevel::decode)),
			muted: req!(try!(remove(&mut value, "muted")).as_boolean()),
		})
	}
}

/// User settings which influence per-server notification behavior
#[derive(Debug, Clone)]
pub struct UserServerSettings {
	pub server_id: ServerId,
	pub message_notifications: NotificationLevel,
	pub mobile_push: bool,
	pub muted: bool,
	pub suppress_everyone: bool,
	pub channel_overrides: Vec<ChannelOverride>,
}

impl UserServerSettings {
	pub fn decode(value: Value) -> Result<UserServerSettings> {
		let mut value = try!(into_map(value));
		warn_json!(value, UserServerSettings {
			server_id: try!(remove(&mut value, "guild_id").and_then(ServerId::decode)),
			message_notifications: try!(remove(&mut value, "message_notifications").and_then(NotificationLevel::decode)),
			mobile_push: req!(try!(remove(&mut value, "mobile_push")).as_boolean()),
			muted: req!(try!(remove(&mut value, "muted")).as_boolean()),
			suppress_everyone: req!(try!(remove(&mut value, "suppress_everyone")).as_boolean()),
			channel_overrides: try!(remove(&mut value, "channel_overrides").and_then(|v| decode_array(v, ChannelOverride::decode))),
		})
	}
}

/// Progress through the Discord tutorial
#[derive(Debug, Clone)]
pub struct Tutorial {
	pub indicators_suppressed: bool,
	pub indicators_confirmed: Vec<String>,
}

impl Tutorial {
	pub fn decode(value: Value) -> Result<Self> {
		let mut value = try!(into_map(value));
		warn_json!(value, Tutorial {
			indicators_suppressed: req!(try!(remove(&mut value, "indicators_suppressed")).as_boolean()),
			indicators_confirmed: try!(remove(&mut value, "indicators_confirmed").and_then(|v| decode_array(v, into_string))),
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
	pub user_settings: Option<UserSettings>,
	pub read_state: Vec<ReadState>,
	pub private_channels: Vec<PrivateChannel>,
	pub servers: Vec<LiveServer>,
	pub user_server_settings: Option<Vec<UserServerSettings>>,
	pub tutorial: Option<Tutorial>,
}

/// Event received over a websocket connection
#[derive(Debug, Clone)]
pub enum Event {
	/// The first event in a connection, containing the initial state
	Ready(ReadyEvent),
	/// Update to the logged-in user's information
	UserUpdate(CurrentUser),
	/// Update to the logged-in user's preferences or client settings
	UserSettingsUpdate {
		enable_tts_command: Option<bool>,
		inline_attachment_media: Option<bool>,
		inline_embed_media: Option<bool>,
		locale: Option<String>,
		message_display_compact: Option<bool>,
		render_embeds: Option<bool>,
		show_current_game: Option<bool>,
		theme: Option<String>,
		convert_emoticons: Option<bool>,
	},
	/// Update to the logged-in user's server-specific notification settings
	UserServerSettingsUpdate(UserServerSettings),
	/// A member's voice state has changed
	VoiceStateUpdate(ServerId, VoiceState),
	/// Voice server information is available
	VoiceServerUpdate {
		server_id: ServerId,
		endpoint: Option<String>,
		token: String,
	},
	/// A user is typing; considered to last 5 seconds
	TypingStart {
		channel_id: ChannelId,
		user_id: UserId,
		timestamp: u64,
	},
	/// A member's presence state (or username or avatar) has changed
	PresenceUpdate {
		server_id: ServerId,
		presence: Presence,
		roles: Vec<RoleId>,
		/// User information if the username or avatar has changed
		user: Option<User>,
	},

	MessageCreate(Message),
	/// A message has been edited, either by the user or the system
	MessageUpdate {
		id: MessageId,
		channel_id: ChannelId,
		content: Option<String>,
		tts: Option<bool>,
		timestamp: Option<String>,
		edited_timestamp: Option<String>,
		author: Option<User>,
		mention_everyone: Option<bool>,
		mentions: Option<Vec<User>>,
		attachments: Option<Vec<Attachment>>,
		embeds: Option<Vec<Value>>,
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

	ServerMemberAdd(ServerId, Member),
	/// A member's roles have changed
	ServerMemberUpdate {
		server_id: ServerId,
		roles: Vec<RoleId>,
		user: User,
	},
	ServerMemberRemove(ServerId, User),

	ServerRoleCreate(ServerId, Role),
	ServerRoleUpdate(ServerId, Role),
	ServerRoleDelete(ServerId, RoleId),

	ServerBanAdd(ServerId, User),
	ServerBanRemove(ServerId, User),

	ServerIntegrationsUpdate(ServerId),
	ServerEmojisUpdate(ServerId, Vec<Emoji>),

	ChannelCreate(Channel),
	ChannelUpdate(Channel),
	ChannelDelete(Channel),

	// Used by Connection internally and turned into GatewayChanged
	#[doc(hidden)]
	_ChangeGateway(String),
	/// The connection's gateway has changed and a new Ready is available
	GatewayChanged(String, ReadyEvent),

	/// An event type not covered by the above
	Unknown(String, BTreeMap<String, Value>),
	/// A websocket "close" frame with the given status
	Closed(u16),
}

impl Event {
	pub fn decode(value: Value) -> Result<Event> {
		let mut value = try!(into_map(value));

		let op = req!(req!(value.remove("op")).as_u64());
		if op == 7 {
			let mut value = try!(remove(&mut value, "d").and_then(into_map));
			return warn_json!(value, Event::_ChangeGateway(
				try!(remove(&mut value, "url").and_then(into_string))
			))
		} else if op != 0 {
			value.insert("op".into(), Value::U64(op));
			return Err(Error::Decode("Expected opcode 7 or 0", Value::Object(value)))
		}

		let kind = try!(remove(&mut value, "t").and_then(into_string));
		let mut value = try!(remove(&mut value, "d").and_then(into_map));
		if kind == "READY" {
			warn_json!(@"Event::Ready", value, Event::Ready(ReadyEvent {
				version: req!(try!(remove(&mut value, "v")).as_u64()),
				user: try!(remove(&mut value, "user").and_then(CurrentUser::decode)),
				session_id: try!(remove(&mut value, "session_id").and_then(into_string)),
				heartbeat_interval: req!(try!(remove(&mut value, "heartbeat_interval")).as_u64()),
				read_state: try!(decode_array(try!(remove(&mut value, "read_state")), ReadState::decode)),
				private_channels: try!(decode_array(try!(remove(&mut value, "private_channels")), PrivateChannel::decode)),
				servers: try!(decode_array(try!(remove(&mut value, "guilds")), LiveServer::decode)),
				user_settings: remove(&mut value, "user_settings").and_then(UserSettings::decode).ok(),
				user_server_settings: remove(&mut value, "user_guild_settings").and_then(|v| decode_array(v, UserServerSettings::decode)).ok(),
				tutorial: remove(&mut value, "tutorial").and_then(Tutorial::decode).ok(),
			}))
		} else if kind == "USER_UPDATE" {
			CurrentUser::decode(Value::Object(value)).map(Event::UserUpdate)
		} else if kind == "USER_SETTINGS_UPDATE" {
			warn_json!(value, Event::UserSettingsUpdate {
				enable_tts_command: remove(&mut value, "enable_tts_command").ok().and_then(|v| v.as_boolean()),
				inline_attachment_media: remove(&mut value, "inline_attachment_media").ok().and_then(|v| v.as_boolean()),
				inline_embed_media: remove(&mut value, "inline_embed_media").ok().and_then(|v| v.as_boolean()),
				locale: remove(&mut value, "locale").and_then(into_string).ok(),
				message_display_compact: remove(&mut value, "message_display_compact").ok().and_then(|v| v.as_boolean()),
				render_embeds: remove(&mut value, "render_embeds").ok().and_then(|v| v.as_boolean()),
				show_current_game: remove(&mut value, "show_current_game").ok().and_then(|v| v.as_boolean()),
				theme: remove(&mut value, "theme").and_then(into_string).ok(),
				convert_emoticons: remove(&mut value, "convert_emoticons").ok().and_then(|v| v.as_boolean()),
			})
		} else if kind == "USER_GUILD_SETTINGS_UPDATE" {
			UserServerSettings::decode(Value::Object(value)).map(Event::UserServerSettingsUpdate)
		} else if kind == "VOICE_STATE_UPDATE" {
			let server_id = try!(remove(&mut value, "guild_id").and_then(ServerId::decode));
			Ok(Event::VoiceStateUpdate(server_id, try!(VoiceState::decode(Value::Object(value)))))
		} else if kind == "VOICE_SERVER_UPDATE" {
			warn_json!(value, Event::VoiceServerUpdate {
				server_id: try!(remove(&mut value, "guild_id").and_then(ServerId::decode)),
				endpoint: remove(&mut value, "endpoint").and_then(into_string).ok(),
				token: try!(remove(&mut value, "token").and_then(into_string)),
			})
		} else if kind == "TYPING_START" {
			warn_json!(value, Event::TypingStart {
				channel_id: try!(remove(&mut value, "channel_id").and_then(ChannelId::decode)),
				user_id: try!(remove(&mut value, "user_id").and_then(UserId::decode)),
				timestamp: req!(try!(remove(&mut value, "timestamp")).as_u64()),
			})
		} else if kind == "PRESENCE_UPDATE" {
			let server_id = try!(remove(&mut value, "guild_id").and_then(ServerId::decode));
			let roles = try!(decode_array(try!(remove(&mut value, "roles")), RoleId::decode));
			let (presence, user) = try!(Presence::decode(Value::Object(value)));
			Ok(Event::PresenceUpdate {
				server_id: server_id,
				roles: roles,
				presence: presence,
				user: user,
			})
		} else if kind == "MESSAGE_CREATE" {
			Message::decode(Value::Object(value)).map(Event::MessageCreate)
		} else if kind == "MESSAGE_UPDATE" {
			warn_json!(value, Event::MessageUpdate {
				id: try!(remove(&mut value, "id").and_then(MessageId::decode)),
				channel_id: try!(remove(&mut value, "channel_id").and_then(ChannelId::decode)),
				content: remove(&mut value, "content").and_then(into_string).ok(),
				tts: remove(&mut value, "tts").ok().and_then(|v| v.as_boolean()),
				timestamp: remove(&mut value, "timestamp").and_then(into_string).ok(),
				edited_timestamp: remove(&mut value, "edited_timestamp").and_then(into_string).ok(),
				author: remove(&mut value, "author").and_then(User::decode).ok(),
				mention_everyone: remove(&mut value, "mention_everyone").ok().and_then(|v| v.as_boolean()),
				mentions: remove(&mut value, "mentions").and_then(|v| decode_array(v, User::decode)).ok(),
				attachments: remove(&mut value, "attachments").and_then(|v| decode_array(v, Attachment::decode)).ok(),
				embeds: remove(&mut value, "embeds").and_then(|v| decode_array(v, Ok)).ok(),
			})
		} else if kind == "MESSAGE_ACK" {
			warn_json!(value, Event::MessageAck {
				message_id: try!(remove(&mut value, "message_id").and_then(MessageId::decode)),
				channel_id: try!(remove(&mut value, "channel_id").and_then(ChannelId::decode)),
			})
		} else if kind == "MESSAGE_DELETE" {
			warn_json!(value, Event::MessageDelete {
				message_id: try!(remove(&mut value, "id").and_then(MessageId::decode)),
				channel_id: try!(remove(&mut value, "channel_id").and_then(ChannelId::decode)),
			})
		} else if kind == "GUILD_CREATE" {
			LiveServer::decode(Value::Object(value)).map(Event::ServerCreate)
		} else if kind == "GUILD_UPDATE" {
			Server::decode(Value::Object(value)).map(Event::ServerUpdate)
		} else if kind == "GUILD_DELETE" {
			Server::decode(Value::Object(value)).map(Event::ServerDelete)
		} else if kind == "GUILD_MEMBER_ADD" {
			Ok(Event::ServerMemberAdd(
				try!(remove(&mut value, "guild_id").and_then(ServerId::decode)),
				try!(Member::decode(Value::Object(value))),
			))
		} else if kind == "GUILD_MEMBER_UPDATE" {
			warn_json!(value, Event::ServerMemberUpdate {
				server_id: try!(remove(&mut value, "guild_id").and_then(ServerId::decode)),
				roles: try!(decode_array(try!(remove(&mut value, "roles")), RoleId::decode)),
				user: try!(remove(&mut value, "user").and_then(User::decode)),
			})
		} else if kind == "GUILD_MEMBER_REMOVE" {
			warn_json!(value, Event::ServerMemberRemove(
				try!(remove(&mut value, "guild_id").and_then(ServerId::decode)),
				try!(remove(&mut value, "user").and_then(User::decode)),
			))
		} else if kind == "GUILD_ROLE_CREATE" {
			warn_json!(value, Event::ServerRoleCreate(
				try!(remove(&mut value, "guild_id").and_then(ServerId::decode)),
				try!(remove(&mut value, "role").and_then(Role::decode)),
			))
		} else if kind == "GUILD_ROLE_UPDATE" {
			warn_json!(value, Event::ServerRoleUpdate(
				try!(remove(&mut value, "guild_id").and_then(ServerId::decode)),
				try!(remove(&mut value, "role").and_then(Role::decode)),
			))
		} else if kind == "GUILD_ROLE_DELETE" {
			warn_json!(value, Event::ServerRoleDelete(
				try!(remove(&mut value, "guild_id").and_then(ServerId::decode)),
				try!(remove(&mut value, "role_id").and_then(RoleId::decode)),
			))
		} else if kind == "GUILD_BAN_ADD" {
			warn_json!(value, Event::ServerBanAdd(
				try!(remove(&mut value, "guild_id").and_then(ServerId::decode)),
				try!(remove(&mut value, "user").and_then(User::decode)),
			))
		} else if kind == "GUILD_BAN_REMOVE" {
			warn_json!(value, Event::ServerBanRemove(
				try!(remove(&mut value, "guild_id").and_then(ServerId::decode)),
				try!(remove(&mut value, "user").and_then(User::decode)),
			))
		} else if kind == "GUILD_INTEGRATIONS_UPDATE" {
			warn_json!(value, Event::ServerIntegrationsUpdate(
				try!(remove(&mut value, "guild_id").and_then(ServerId::decode)),
			))
		} else if kind == "GUILD_EMOJIS_UPDATE" {
			warn_json!(value, Event::ServerEmojisUpdate(
				try!(remove(&mut value, "guild_id").and_then(ServerId::decode)),
				try!(remove(&mut value, "emojis").and_then(|v| decode_array(v, Emoji::decode))),
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
// Voice event model
#[doc(hidden)]
#[derive(Debug, Clone)]
pub enum VoiceEvent {
	Handshake {
		heartbeat_interval: u64,
		port: u16,
		ssrc: u32,
		modes: Vec<String>,
	},
	Ready {
		mode: String,
		secret_key: Vec<u8>,
	},
	SpeakingUpdate {
		user_id: UserId,
		ssrc: u32,
		speaking: bool,
	},
	KeepAlive,
	Unknown(u64, Value)
}

impl VoiceEvent {
	pub fn decode(value: Value) -> Result<VoiceEvent> {
		let mut value = try!(into_map(value));

		let op = req!(req!(value.remove("op")).as_u64());
		if op == 3 {
			return Ok(VoiceEvent::KeepAlive)
		}

		let mut value = try!(remove(&mut value, "d").and_then(into_map));
		if op == 2 {
			warn_json!(value, VoiceEvent::Handshake {
				heartbeat_interval: req!(try!(remove(&mut value, "heartbeat_interval")).as_u64()),
				modes: try!(decode_array(try!(remove(&mut value, "modes")), into_string)),
				port: req!(try!(remove(&mut value, "port")).as_u64()) as u16,
				ssrc: req!(try!(remove(&mut value, "ssrc")).as_u64()) as u32,
			})
		} else if op == 4 {
			warn_json!(value, VoiceEvent::Ready {
				mode: try!(remove(&mut value, "mode").and_then(into_string)),
				secret_key: try!(decode_array(try!(remove(&mut value, "secret_key")),
					|v| Ok(req!(v.as_u64()) as u8)
				)),
			})
		} else if op == 5 {
			warn_json!(value, VoiceEvent::SpeakingUpdate {
				user_id: try!(remove(&mut value, "user_id").and_then(UserId::decode)),
				ssrc: req!(try!(remove(&mut value, "ssrc")).as_u64()) as u32,
				speaking: req!(try!(remove(&mut value, "speaking")).as_boolean()),
			})
		} else {
			Ok(VoiceEvent::Unknown(op, Value::Object(value)))
		}
	}
}

//=================
// Decode helpers

fn remove(map: &mut BTreeMap<String, Value>, key: &str) -> Result<Value> {
	map.remove(key).ok_or(Error::Decode("Unexpected absent key", Value::String(key.into())))
}

fn decode_discriminator(value: Value) -> Result<String> {
	match value {
		Value::String(s) => Ok(s),
		Value::I64(v) => Ok(v.to_string()),
		Value::U64(v) => Ok(v.to_string()),
		other => Err(Error::Decode("Expected string or u64", other))
	}
}

fn into_string(value: Value) -> Result<String> {
	match value {
		Value::String(s) => Ok(s),
		value => Err(Error::Decode("Expected string", value)),
	}
}

fn into_array(value: Value) -> Result<Vec<Value>> {
	match value {
		Value::Array(v) => Ok(v),
		value => Err(Error::Decode("Expected array", value)),
	}
}

fn into_map(value: Value) -> Result<BTreeMap<String, Value>> {
	match value {
		Value::Object(m) => Ok(m),
		value => Err(Error::Decode("Expected object", value)),
	}
}

#[doc(hidden)]
pub fn decode_array<T, F: Fn(Value) -> Result<T>>(value: Value, f: F) -> Result<Vec<T>> {
	into_array(value).and_then(|x| x.into_iter().map(f).collect())
}

fn warn_field(name: &str, map: BTreeMap<String, Value>) {
	if map.len() != 0 {
		debug!("Unhandled keys: {} has {:?}", name, Value::Object(map))
	}
}
