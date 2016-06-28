//! Struct and enum definitions of values in the Discord model.
#![allow(missing_docs)]

use super::{Error, Result};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fmt;

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
	match value {
		Value::U64(num) => Ok(num),
		Value::String(text) => match text.parse::<u64>() {
			Ok(num) => Ok(num),
			Err(_) => Err(Error::Decode("Expected numeric ID", Value::String(text)))
		},
		value => Err(Error::Decode("Expected numeric ID", value))
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

/// A mention targeted at a specific user, channel, or other entity.
///
/// A mention can be constructed by calling `.mention()` on a mentionable item
/// or an ID type which refers to it, and can be formatted into a string using
/// the `format!` macro:
///
/// ```ignore
/// let message = format!("Hey, {}, ping!", user.mention());
/// ```
///
/// If a `String` is required, call `mention.to_string()`.
pub struct Mention {
	prefix: &'static str,
	id: u64,
}

impl fmt::Display for Mention {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		try!(f.write_str(self.prefix));
		try!(fmt::Display::fmt(&self.id, f));
		fmt::Write::write_char(f, '>')
	}
}

impl UserId {
	/// Return a `Mention` which will ping this user.
	#[inline(always)]
	pub fn mention(&self) -> Mention {
		Mention { prefix: "<@", id: self.0 }
	}
}

impl RoleId {
	/// Return a `Mention` which will ping members of this role.
	#[inline(always)]
	pub fn mention(&self) -> Mention {
		Mention { prefix: "<@&", id: self.0 }
	}
}

impl ChannelId {
	/// Return a `Mention` which will link to this channel.
	#[inline(always)]
	pub fn mention(&self) -> Mention {
		Mention { prefix: "<#", id: self.0 }
	}
}

#[test]
fn mention_test() {
	assert_eq!(UserId(1234).mention().to_string(), "<@1234>");
	assert_eq!(RoleId(1234).mention().to_string(), "<@&1234>");
	assert_eq!(ChannelId(1234).mention().to_string(), "<#1234>");
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
	pub permissions: Permissions,
}

impl ServerInfo {
	pub fn decode(value: Value) -> Result<Self> {
		let mut value = try!(into_map(value));
		warn_json!(value, ServerInfo {
			id: try!(remove(&mut value, "id").and_then(ServerId::decode)),
			name: try!(remove(&mut value, "name").and_then(into_string)),
			icon: try!(opt(&mut value, "icon", into_string)),
			owner: req!(try!(remove(&mut value, "owner")).as_boolean()),
			permissions: try!(remove(&mut value, "permissions").and_then(Permissions::decode)),
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
	pub emojis: Vec<Emoji>,
	pub features: Vec<String>,
	pub splash: Option<String>,
	pub default_message_notifications: u64,
	pub mfa_level: u64,
}

impl Server {
	pub fn decode(value: Value) -> Result<Server> {
		let mut value = try!(into_map(value));
		warn_json!(value, Server {
			id: try!(remove(&mut value, "id").and_then(ServerId::decode)),
			name: try!(remove(&mut value, "name").and_then(into_string)),
			icon: try!(opt(&mut value, "icon", into_string)),
			afk_timeout: req!(try!(remove(&mut value, "afk_timeout")).as_u64()),
			afk_channel_id: try!(opt(&mut value, "afk_channel_id", ChannelId::decode)),
			embed_enabled: req!(try!(remove(&mut value, "embed_enabled")).as_boolean()),
			embed_channel_id: try!(opt(&mut value, "embed_channel_id", ChannelId::decode)),
			owner_id: try!(remove(&mut value, "owner_id").and_then(UserId::decode)),
			region: try!(remove(&mut value, "region").and_then(into_string)),
			roles: try!(decode_array(try!(remove(&mut value, "roles")), Role::decode)),
			verification_level: try!(remove(&mut value, "verification_level").and_then(VerificationLevel::decode)),
			emojis: try!(remove(&mut value, "emojis").and_then(|v| decode_array(v, Emoji::decode))),
			features: try!(remove(&mut value, "features").and_then(|v| decode_array(v, into_string))),
			splash: try!(opt(&mut value, "splash", into_string)),
			default_message_notifications: req!(try!(remove(&mut value, "default_message_notifications")).as_u64()),
			mfa_level: req!(try!(remove(&mut value, "mfa_level")).as_u64()),
		})
	}
}

/// Representation of the number of member that would be pruned by a server
/// prune operation.
#[derive(Debug, Clone)]
pub struct ServerPrune {
	pub pruned: u64,
}

impl ServerPrune {
	pub fn decode(value: Value) -> Result<ServerPrune> {
		let mut value = try!(into_map(value));
		warn_json!(value, ServerPrune {
			pruned: req!(try!(remove(&mut value, "pruned")).as_u64()),
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
	pub mentionable: bool,
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
			mentionable: try!(opt(&mut value, "mentionable", |v| Ok(req!(v.as_boolean())))).unwrap_or(false),
		})
	}

	/// Return a `Mention` which will ping members of this role.
	#[inline(always)]
	pub fn mention(&self) -> Mention { self.id.mention() }
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
			avatar: try!(opt(&mut value, "avatar", into_string)),
			bot: try!(opt(&mut value, "bot", |v| Ok(req!(v.as_boolean())))).unwrap_or(false),
		})
	}

	#[doc(hidden)]
	pub fn decode_ban(value: Value) -> Result<User> {
		let mut value = try!(into_map(value));
		warn_json!(@"Ban", value, try!(remove(&mut value, "user").and_then(User::decode)))
	}

	/// Return a `Mention` which will ping this user.
	#[inline(always)]
	pub fn mention(&self) -> Mention { self.id.mention() }
}

/// Information about a member of a server
#[derive(Debug, Clone)]
pub struct Member {
	pub user: User,
	pub roles: Vec<RoleId>,
	pub nick: Option<String>,
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
			nick: try!(opt(&mut value, "nick", into_string)),
			joined_at: try!(remove(&mut value, "joined_at").and_then(into_string)),
			mute: req!(try!(remove(&mut value, "mute")).as_boolean()),
			deaf: req!(try!(remove(&mut value, "deaf")).as_boolean()),
		})
	}

	pub fn display_name(&self) -> &str {
		if let Some(name) = self.nick.as_ref() {
			name
		} else {
			&self.user.name
		}
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
			last_message_id: try!(opt(&mut value, "last_message_id", MessageId::decode)),
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
	pub user_limit: Option<u64>,
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
			topic: try!(opt(&mut value, "topic", into_string)),
			position: req!(try!(remove(&mut value, "position")).as_i64()),
			kind: try!(remove(&mut value, "type").and_then(into_string).and_then(ChannelType::from_str_err)),
			last_message_id: try!(opt(&mut value, "last_message_id", MessageId::decode)),
			permission_overwrites: try!(decode_array(try!(remove(&mut value, "permission_overwrites")), PermissionOverwrite::decode)),
			bitrate: remove(&mut value, "bitrate").ok().and_then(|v| v.as_u64()),
			user_limit: remove(&mut value, "user_limit").ok().and_then(|v| v.as_u64()),
		})
	}

	/// Return a `Mention` which will link to this channel.
	#[inline(always)]
	pub fn mention(&self) -> Mention { self.id.mention() }
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
			/// Change their own nickname
			const CHANGE_NICKNAMES = 1 << 26,
			/// Change the nickname of other users
			const MANAGE_NICKNAMES = 1 << 27,

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
	pub pinned: bool,

	pub author: User,
	pub mention_everyone: bool,
	pub mentions: Vec<User>,
	pub mention_roles: Vec<RoleId>,

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
			nonce: remove(&mut value, "nonce").and_then(into_string).ok(), // nb: swallow errors
			content: try!(remove(&mut value, "content").and_then(into_string)),
			tts: req!(try!(remove(&mut value, "tts")).as_boolean()),
			timestamp: try!(remove(&mut value, "timestamp").and_then(into_string)),
			edited_timestamp: try!(opt(&mut value, "edited_timestamp", into_string)),
			pinned: req!(try!(remove(&mut value, "pinned")).as_boolean()),
			mention_everyone: req!(try!(remove(&mut value, "mention_everyone")).as_boolean()),
			mentions: try!(decode_array(try!(remove(&mut value, "mentions")), User::decode)),
			mention_roles: try!(decode_array(try!(remove(&mut value, "mention_roles")), RoleId::decode)),
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
			xkcdpass: try!(opt(&mut value, "xkcdpass", into_string)),
			server_id: server_id,
			server_name: server_name,
			channel_type: channel_type,
			channel_id: channel_id,
			channel_name: channel_name,
		})
	}
}

/// Detailed information about an invite, available to server managers
#[derive(Debug, Clone)]
pub struct RichInvite {
	pub code: String,
	pub xkcdpass: Option<String>,
	pub server_id: ServerId,
	pub server_name: String,
	pub server_splash_hash: Option<String>,
	pub channel_type: ChannelType,
	pub channel_id: ChannelId,
	pub channel_name: String,
	pub inviter: User,
	pub created_at: String,
	pub max_age: u64,
	pub max_uses: u64,
	pub revoked: bool,
	pub temporary: bool,
	pub uses: u64,
}

impl RichInvite {
	pub fn decode(value: Value) -> Result<Self> {
		let mut value = try!(into_map(value));

		let mut server = try!(remove(&mut value, "guild").and_then(into_map));
		let server_id = try!(remove(&mut server, "id").and_then(ServerId::decode));
		let server_name = try!(remove(&mut server, "name").and_then(into_string));
		let server_splash_hash = try!(opt(&mut server, "splash_hash", into_string));
		warn_field("RichInvite/guild", server);

		let mut channel = try!(remove(&mut value, "channel").and_then(into_map));
		let channel_type = try!(remove(&mut channel, "type").and_then(into_string).and_then(ChannelType::from_str_err));
		let channel_id = try!(remove(&mut channel, "id").and_then(ChannelId::decode));
		let channel_name = try!(remove(&mut channel, "name").and_then(into_string));
		warn_field("RichInvite/channel", channel);

		warn_json!(value, RichInvite {
			code: try!(remove(&mut value, "code").and_then(into_string)),
			xkcdpass: try!(opt(&mut value, "xkcdpass", into_string)),
			server_id: server_id,
			server_name: server_name,
			server_splash_hash: server_splash_hash,
			channel_type: channel_type,
			channel_id: channel_id,
			channel_name: channel_name,
			inviter: try!(remove(&mut value, "inviter").and_then(User::decode)),
			created_at: try!(remove(&mut value, "created_at").and_then(into_string)),
			max_age: req!(try!(remove(&mut value, "max_age")).as_u64()),
			max_uses: req!(try!(remove(&mut value, "max_uses")).as_u64()),
			revoked: req!(try!(remove(&mut value, "revoked")).as_boolean()),
			temporary: req!(try!(remove(&mut value, "temporary")).as_boolean()),
			uses: req!(try!(remove(&mut value, "uses")).as_u64()),
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
			last_message_id: try!(opt(&mut value, "last_message_id", MessageId::decode)),
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

/// A type of game being played.
#[derive(Copy, Clone, Hash, Eq, PartialEq, Debug)]
pub enum GameType {
	Playing,
	Streaming,
}

impl GameType {
	pub fn from_num(num: u64) -> Option<Self> {
		match num {
			0 => Some(GameType::Playing),
			1 => Some(GameType::Streaming),
			_ => None,
		}
	}

	fn decode(value: Value) -> Result<Self> {
		value.as_u64().and_then(GameType::from_num).ok_or(Error::Decode("Expected valid GameType", value))
	}
}

/// Information about a game being played
#[derive(Debug, Clone)]
pub struct Game {
	pub name: String,
	pub url: Option<String>,
	pub kind: GameType,
}

impl Game {
	pub fn playing(name: String) -> Game {
		Game { kind: GameType::Playing, name: name, url: None }
	}

	pub fn streaming(name: String, url: String) -> Game {
		Game { kind: GameType::Streaming, name: name, url: Some(url) }
	}

	pub fn decode(value: Value) -> Result<Option<Game>> {
		let mut value = try!(into_map(value));
		let name = match value.remove("name") {
			None | Some(Value::Null) => return Ok(None),
			Some(val) => try!(into_string(val)),
		};
		if name.trim().len() == 0 {
			return Ok(None)
		}
		warn_json!(@"Game", value, Some(Game {
			name: name,
			kind: try!(opt(&mut value, "type", GameType::decode)).unwrap_or(GameType::Playing),
			url: try!(opt(&mut value, "url", into_string)),
		}))
	}
}

/// A members's online status
#[derive(Debug, Clone)]
pub struct Presence {
	pub user_id: UserId,
	pub status: OnlineStatus,
	pub last_modified: Option<u64>,
	pub game: Option<Game>,
	pub user: Option<User>,
	pub nick: Option<String>,
}

impl Presence {
	pub fn decode(value: Value) -> Result<Presence> {
		let mut value = try!(into_map(value));
		let mut user_map = try!(remove(&mut value, "user").and_then(into_map));

		let (user_id, user) = if user_map.len() > 1 {
			let user = try!(User::decode(Value::Object(user_map)));
			(user.id.clone(), Some(user))
		} else {
			(try!(remove(&mut user_map, "id").and_then(UserId::decode)), None)
		};

		warn_json!(@"Presence", value, Presence {
			user_id: user_id,
			status: try!(remove(&mut value, "status").and_then(into_string).and_then(OnlineStatus::from_str_err)),
			last_modified: try!(opt(&mut value, "last_modified", |v| Ok(req!(v.as_u64())))),
			game: match value.remove("game") {
				None | Some(Value::Null) => None,
				Some(val) => try!(Game::decode(val)),
			},
			user: user,
			nick: try!(opt(&mut value, "nick", into_string)),
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
			user_id: try!(remove(&mut value, "user_id").and_then(UserId::decode)),
			channel_id: try!(opt(&mut value, "channel_id", ChannelId::decode)),
			session_id: try!(remove(&mut value, "session_id").and_then(into_string)),
			token: try!(opt(&mut value, "token", into_string)),
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

	pub fn to_num(self) -> u64 {
		match self {
			VerificationLevel::None => 0,
			VerificationLevel::Low => 1,
			VerificationLevel::Medium => 2,
			VerificationLevel::High => 3,
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
	pub default_message_notifications: u64,
	pub mfa_level: u64,
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
			presences: try!(decode_array(try!(remove(&mut value, "presences")), Presence::decode)),
			member_count: req!(try!(remove(&mut value, "member_count")).as_u64()),
			members: try!(decode_array(try!(remove(&mut value, "members")), Member::decode)),
			joined_at: try!(remove(&mut value, "joined_at").and_then(into_string)),
			icon: try!(opt(&mut value, "icon", into_string)),
			large: req!(try!(remove(&mut value, "large")).as_boolean()),
			afk_timeout: req!(try!(remove(&mut value, "afk_timeout")).as_u64()),
			afk_channel_id: try!(opt(&mut value, "afk_channel_id", ChannelId::decode)),
			channels: try!(decode_array(try!(remove(&mut value, "channels")), |v| PublicChannel::decode_server(v, id.clone()))),
			verification_level: try!(remove(&mut value, "verification_level").and_then(VerificationLevel::decode)),
			emojis: try!(remove(&mut value, "emojis").and_then(|v| decode_array(v, Emoji::decode))),
			features: try!(remove(&mut value, "features").and_then(|v| decode_array(v, into_string))),
			splash: try!(opt(&mut value, "splash", into_string)),
			default_message_notifications: req!(try!(remove(&mut value, "default_message_notifications")).as_u64()),
			mfa_level: req!(try!(remove(&mut value, "mfa_level")).as_u64()),
			id: id,
		})
	}
}

/// A server which may be unavailable
#[derive(Debug, Clone)]
pub enum PossibleServer<T> {
	/// An offline server, the ID of which is known
	Offline(ServerId),
	/// An online server, for which more information is available
	Online(T),
}

impl PossibleServer<LiveServer> {
	pub fn decode(value: Value) -> Result<Self> {
		let mut value = try!(into_map(value));
		if remove(&mut value, "unavailable").ok().and_then(|v| v.as_boolean()).unwrap_or(false) {
			remove(&mut value, "id").and_then(ServerId::decode).map(PossibleServer::Offline)
		} else {
			LiveServer::decode(Value::Object(value)).map(PossibleServer::Online)
		}
	}

	pub fn id(&self) -> ServerId {
		match *self {
			PossibleServer::Offline(id) => id,
			PossibleServer::Online(ref ls) => ls.id,
		}
	}
}

impl PossibleServer<Server> {
	pub fn decode(value: Value) -> Result<Self> {
		let mut value = try!(into_map(value));
		if remove(&mut value, "unavailable").ok().and_then(|v| v.as_boolean()).unwrap_or(false) {
			remove(&mut value, "id").and_then(ServerId::decode).map(PossibleServer::Offline)
		} else {
			Server::decode(Value::Object(value)).map(PossibleServer::Online)
		}
	}

	pub fn id(&self) -> ServerId {
		match *self {
			PossibleServer::Offline(id) => id,
			PossibleServer::Online(ref ls) => ls.id,
		}
	}
}

/// Information about the logged-in user
#[derive(Debug, Clone)]
pub struct CurrentUser {
	pub id: UserId,
	pub username: String,
	pub discriminator: String,
	pub avatar: Option<String>,
	pub email: Option<String>,
	pub verified: bool,
	pub bot: bool,
	pub mfa_enabled: bool,
}

impl CurrentUser {
	pub fn decode(value: Value) -> Result<CurrentUser> {
		let mut value = try!(into_map(value));
		warn_json!(value, CurrentUser {
			id: try!(remove(&mut value, "id").and_then(UserId::decode)),
			username: try!(remove(&mut value, "username").and_then(into_string)),
			discriminator: try!(remove(&mut value, "discriminator").and_then(into_string)),
			email: try!(opt(&mut value, "email", into_string)),
			avatar: try!(opt(&mut value, "avatar", into_string)),
			verified: req!(try!(remove(&mut value, "verified")).as_boolean()),
			bot: try!(opt(&mut value, "bot", |v| Ok(req!(v.as_boolean())))).unwrap_or(false),
			mfa_enabled: req!(try!(remove(&mut value, "mfa_enabled")).as_boolean()),
		})
	}
}

/// A type of relationship this user has with another.
#[derive(Copy, Clone, Hash, Eq, PartialEq, Debug)]
pub enum RelationshipType {
	Ignored,
	Friends,
	Blocked,
	IncomingRequest,
	OutgoingRequest,
}

impl RelationshipType {
	pub fn from_num(kind: u64) -> Option<Self> {
		match kind {
			0 => Some(RelationshipType::Ignored),
			1 => Some(RelationshipType::Friends),
			2 => Some(RelationshipType::Blocked),
			3 => Some(RelationshipType::IncomingRequest),
			4 => Some(RelationshipType::OutgoingRequest),
			_ => None,
		}
	}

	fn decode(value: Value) -> Result<Self> {
		value.as_u64().and_then(RelationshipType::from_num).ok_or(Error::Decode("Expected valid RelationshipType", value))
	}
}

/// Information on a friendship relationship this user has with another.
#[derive(Debug, Clone)]
pub struct Relationship {
	pub id: UserId,
	pub kind: RelationshipType,
	pub user: User,
}

impl Relationship {
	pub fn decode(value: Value) -> Result<Self> {
		let mut value = try!(into_map(value));
		warn_json!(value, Relationship {
			id: try!(remove(&mut value, "id").and_then(UserId::decode)),
			kind: try!(remove(&mut value, "type").and_then(RelationshipType::decode)),
			user: try!(remove(&mut value, "user").and_then(User::decode)),
		})
	}
}

/// Flags for who may add this user as a friend.
#[derive(Debug, Clone)]
pub struct FriendSourceFlags {
	pub all: bool,
	pub mutual_friends: bool,
	pub mutual_servers: bool,
}

impl FriendSourceFlags {
	pub fn decode(value: Value) -> Result<Self> {
		let mut value = try!(into_map(value));
		warn_json!(value, FriendSourceFlags {
			all: try!(opt(&mut value, "all", |v| Ok(req!(v.as_boolean())))).unwrap_or(false),
			mutual_friends: try!(opt(&mut value, "mutual_friends", |v| Ok(req!(v.as_boolean())))).unwrap_or(false),
			mutual_servers: try!(opt(&mut value, "mutual_guilds", |v| Ok(req!(v.as_boolean())))).unwrap_or(false),
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
	pub allow_email_friend_request: bool,
	pub friend_source_flags: FriendSourceFlags,
	/// Servers whose members cannot private message this user.
	pub restricted_servers: Vec<ServerId>,
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
			allow_email_friend_request: req!(try!(remove(&mut value, "allow_email_friend_request")).as_boolean()),
			friend_source_flags: try!(remove(&mut value, "friend_source_flags").and_then(FriendSourceFlags::decode)),
			restricted_servers: try!(remove(&mut value, "restricted_guilds").and_then(|v| decode_array(v, ServerId::decode))),
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

/// Discord status maintenance message.
///
/// This can be either for active maintenances or scheduled maintenances.
#[derive(Debug, Clone)]
pub struct Maintenance {
	pub description: String,
	pub id: String,
	pub name: String,
	pub start: String,
	pub stop: String,
}

impl Maintenance {
	pub fn decode(value: Value) -> Result<Self> {
		let mut value = try!(into_map(value));
		warn_json!(value, Maintenance {
			description: try!(remove(&mut value, "description").and_then(into_string)),
			id: try!(remove(&mut value, "id").and_then(into_string)),
			name: try!(remove(&mut value, "name").and_then(into_string)),
			start: try!(remove(&mut value, "start").and_then(into_string)),
			stop: try!(remove(&mut value, "stop").and_then(into_string)),
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
	pub read_state: Option<Vec<ReadState>>,
	pub private_channels: Vec<PrivateChannel>,
	pub presences: Vec<Presence>,
	pub relationships: Vec<Relationship>,
	pub servers: Vec<PossibleServer<LiveServer>>,
	pub user_server_settings: Option<Vec<UserServerSettings>>,
	pub tutorial: Option<Tutorial>,
	/// The trace of servers involved in this connection.
	pub trace: Vec<Option<String>>,
	pub notes: Option<BTreeMap<UserId, String>>,
}

/// Event received over a websocket connection
#[derive(Debug, Clone)]
pub enum Event {
	/// The first event in a connection, containing the initial state.
	///
	/// May also be received at a later time in the event of a reconnect.
	Ready(ReadyEvent),
	/// The connection has successfully resumed after a disconnect.
	Resumed {
		heartbeat_interval: u64,
		trace: Vec<Option<String>>,
	},

	/// Update to the logged-in user's information
	UserUpdate(CurrentUser),
	/// Update to a note that the logged-in user has set for another user.
	UserNoteUpdate(UserId, String),
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
		allow_email_friend_request: Option<bool>,
		friend_source_flags: Option<FriendSourceFlags>,
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
		presence: Presence,
		server_id: Option<ServerId>,
		roles: Option<Vec<RoleId>>,
	},
	/// The precense list of the user's friends should be replaced entirely
	PresencesReplace(Vec<Presence>),
	RelationshipAdd(Relationship),
	RelationshipRemove(UserId, RelationshipType),

	MessageCreate(Message),
	/// A message has been edited, either by the user or the system
	MessageUpdate {
		id: MessageId,
		channel_id: ChannelId,
		content: Option<String>,
		nonce: Option<String>,
		tts: Option<bool>,
		pinned: Option<bool>,
		timestamp: Option<String>,
		edited_timestamp: Option<String>,
		author: Option<User>,
		mention_everyone: Option<bool>,
		mentions: Option<Vec<User>>,
		mention_roles: Option<Vec<RoleId>>,
		attachments: Option<Vec<Attachment>>,
		embeds: Option<Vec<Value>>,
	},
	/// Another logged-in device acknowledged this message
	MessageAck {
		channel_id: ChannelId,
		/// May be `None` if a private channel with no messages has closed.
		message_id: Option<MessageId>,
	},
	MessageDelete {
		channel_id: ChannelId,
		message_id: MessageId,
	},

	ServerCreate(PossibleServer<LiveServer>),
	ServerUpdate(Server),
	ServerDelete(PossibleServer<Server>),

	ServerMemberAdd(ServerId, Member),
	/// A member's roles have changed
	ServerMemberUpdate {
		server_id: ServerId,
		roles: Vec<RoleId>,
		user: User,
		nick: Option<String>,
	},
	ServerMemberRemove(ServerId, User),
	ServerMembersChunk(ServerId, Vec<Member>),

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

	/// An event type not covered by the above
	Unknown(String, BTreeMap<String, Value>),
	// Any other event. Should never be used directly.
	#[doc(hidden)]
	__Nonexhaustive,
}

impl Event {
	fn decode(kind: String, value: Value) -> Result<Event> {
		if kind == "PRESENCES_REPLACE" {
			return decode_array(value, Presence::decode).map(Event::PresencesReplace);
		}
		let mut value = try!(into_map(value));
		if kind == "READY" {
			warn_json!(@"Event::Ready", value, Event::Ready(ReadyEvent {
				version: req!(try!(remove(&mut value, "v")).as_u64()),
				user: try!(remove(&mut value, "user").and_then(CurrentUser::decode)),
				session_id: try!(remove(&mut value, "session_id").and_then(into_string)),
				heartbeat_interval: req!(try!(remove(&mut value, "heartbeat_interval")).as_u64()),
				read_state: try!(opt(&mut value, "read_state", |v| decode_array(v, ReadState::decode))),
				private_channels: try!(decode_array(try!(remove(&mut value, "private_channels")), PrivateChannel::decode)),
				presences: try!(decode_array(try!(remove(&mut value, "presences")), Presence::decode)),
				relationships: try!(remove(&mut value, "relationships").and_then(|v| decode_array(v, Relationship::decode))),
				servers: try!(decode_array(try!(remove(&mut value, "guilds")), PossibleServer::<LiveServer>::decode)),
				user_settings: try!(opt(&mut value, "user_settings", UserSettings::decode)),
				user_server_settings: try!(opt(&mut value, "user_guild_settings", |v| decode_array(v, UserServerSettings::decode))),
				tutorial: try!(opt(&mut value, "tutorial", Tutorial::decode)),
				notes: try!(opt(&mut value, "notes", decode_notes)),
				trace: try!(remove(&mut value, "_trace").and_then(|v| decode_array(v, |v| Ok(into_string(v).ok())))),
			}))
		} else if kind == "RESUMED" {
			warn_json!(value, Event::Resumed {
				heartbeat_interval: req!(try!(remove(&mut value, "heartbeat_interval")).as_u64()),
				trace: try!(remove(&mut value, "_trace").and_then(|v| decode_array(v, |v| Ok(into_string(v).ok())))),
			})
		} else if kind == "USER_UPDATE" {
			CurrentUser::decode(Value::Object(value)).map(Event::UserUpdate)
		} else if kind == "USER_NOTE_UPDATE" {
			warn_json!(value, Event::UserNoteUpdate(
				try!(remove(&mut value, "id").and_then(UserId::decode)),
				try!(remove(&mut value, "note").and_then(into_string)),
			))
		} else if kind == "USER_SETTINGS_UPDATE" {
			warn_json!(value, Event::UserSettingsUpdate {
				enable_tts_command: remove(&mut value, "enable_tts_command").ok().and_then(|v| v.as_boolean()),
				inline_attachment_media: remove(&mut value, "inline_attachment_media").ok().and_then(|v| v.as_boolean()),
				inline_embed_media: remove(&mut value, "inline_embed_media").ok().and_then(|v| v.as_boolean()),
				locale: try!(opt(&mut value, "locale", into_string)),
				message_display_compact: remove(&mut value, "message_display_compact").ok().and_then(|v| v.as_boolean()),
				render_embeds: remove(&mut value, "render_embeds").ok().and_then(|v| v.as_boolean()),
				show_current_game: remove(&mut value, "show_current_game").ok().and_then(|v| v.as_boolean()),
				theme: try!(opt(&mut value, "theme", into_string)),
				convert_emoticons: remove(&mut value, "convert_emoticons").ok().and_then(|v| v.as_boolean()),
				allow_email_friend_request: remove(&mut value, "allow_email_friend_request").ok().and_then(|v| v.as_boolean()),
				friend_source_flags: try!(opt(&mut value, "friend_source_flags", FriendSourceFlags::decode)),
			})
		} else if kind == "USER_GUILD_SETTINGS_UPDATE" {
			UserServerSettings::decode(Value::Object(value)).map(Event::UserServerSettingsUpdate)
		} else if kind == "VOICE_STATE_UPDATE" {
			let server_id = try!(remove(&mut value, "guild_id").and_then(ServerId::decode));
			Ok(Event::VoiceStateUpdate(server_id, try!(VoiceState::decode(Value::Object(value)))))
		} else if kind == "VOICE_SERVER_UPDATE" {
			warn_json!(value, Event::VoiceServerUpdate {
				server_id: try!(remove(&mut value, "guild_id").and_then(ServerId::decode)),
				endpoint: try!(opt(&mut value, "endpoint", into_string)),
				token: try!(remove(&mut value, "token").and_then(into_string)),
			})
		} else if kind == "TYPING_START" {
			warn_json!(value, Event::TypingStart {
				channel_id: try!(remove(&mut value, "channel_id").and_then(ChannelId::decode)),
				user_id: try!(remove(&mut value, "user_id").and_then(UserId::decode)),
				timestamp: req!(try!(remove(&mut value, "timestamp")).as_u64()),
			})
		} else if kind == "PRESENCE_UPDATE" {
			let server_id = try!(opt(&mut value, "guild_id", ServerId::decode));
			let roles = try!(opt(&mut value, "roles", |v| decode_array(v, RoleId::decode)));
			let presence = try!(Presence::decode(Value::Object(value)));
			Ok(Event::PresenceUpdate {
				server_id: server_id,
				roles: roles,
				presence: presence,
			})
		} else if kind == "RELATIONSHIP_ADD" {
			Relationship::decode(Value::Object(value)).map(Event::RelationshipAdd)
		} else if kind == "RELATIONSHIP_REMOVE" {
			warn_json!(value, Event::RelationshipRemove(
				try!(remove(&mut value, "id").and_then(UserId::decode)),
				try!(remove(&mut value, "type").and_then(RelationshipType::decode)),
			))
		} else if kind == "MESSAGE_CREATE" {
			Message::decode(Value::Object(value)).map(Event::MessageCreate)
		} else if kind == "MESSAGE_UPDATE" {
			warn_json!(value, Event::MessageUpdate {
				id: try!(remove(&mut value, "id").and_then(MessageId::decode)),
				channel_id: try!(remove(&mut value, "channel_id").and_then(ChannelId::decode)),
				content: try!(opt(&mut value, "content", into_string)),
				nonce: remove(&mut value, "nonce").and_then(into_string).ok(), // nb: swallow errors
				tts: remove(&mut value, "tts").ok().and_then(|v| v.as_boolean()),
				pinned: remove(&mut value, "pinned").ok().and_then(|v| v.as_boolean()),
				timestamp: try!(opt(&mut value, "timestamp", into_string)),
				edited_timestamp: try!(opt(&mut value, "edited_timestamp", into_string)),
				author: try!(opt(&mut value, "author", User::decode)),
				mention_everyone: remove(&mut value, "mention_everyone").ok().and_then(|v| v.as_boolean()),
				mentions: try!(opt(&mut value, "mentions", |v| decode_array(v, User::decode))),
				mention_roles: try!(opt(&mut value, "mention_roles", |v| decode_array(v, RoleId::decode))),
				attachments: try!(opt(&mut value, "attachments", |v| decode_array(v, Attachment::decode))),
				embeds: try!(opt(&mut value, "embeds", |v| decode_array(v, Ok))),
			})
		} else if kind == "MESSAGE_ACK" {
			warn_json!(value, Event::MessageAck {
				channel_id: try!(remove(&mut value, "channel_id").and_then(ChannelId::decode)),
				message_id: try!(opt(&mut value, "message_id", MessageId::decode)),
			})
		} else if kind == "MESSAGE_DELETE" {
			warn_json!(value, Event::MessageDelete {
				channel_id: try!(remove(&mut value, "channel_id").and_then(ChannelId::decode)),
				message_id: try!(remove(&mut value, "id").and_then(MessageId::decode)),
			})
		} else if kind == "GUILD_CREATE" {
			PossibleServer::<LiveServer>::decode(Value::Object(value)).map(Event::ServerCreate)
		} else if kind == "GUILD_UPDATE" {
			Server::decode(Value::Object(value)).map(Event::ServerUpdate)
		} else if kind == "GUILD_DELETE" {
			PossibleServer::<Server>::decode(Value::Object(value)).map(Event::ServerDelete)
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
				nick: try!(opt(&mut value, "nick", into_string)),
			})
		} else if kind == "GUILD_MEMBER_REMOVE" {
			warn_json!(value, Event::ServerMemberRemove(
				try!(remove(&mut value, "guild_id").and_then(ServerId::decode)),
				try!(remove(&mut value, "user").and_then(User::decode)),
			))
		} else if kind == "GUILD_MEMBERS_CHUNK" {
			warn_json!(value, Event::ServerMembersChunk(
				try!(remove(&mut value, "guild_id").and_then(ServerId::decode)),
				try!(remove(&mut value, "members").and_then(|v| decode_array(v, Member::decode))),
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

#[doc(hidden)]
#[derive(Debug, Clone)]
pub enum GatewayEvent {
	Dispatch(u64, Event),
	Heartbeat(u64),
	Reconnect,
	InvalidateSession,
}

impl GatewayEvent {
	pub fn decode(value: Value) -> Result<Self> {
		let mut value = try!(into_map(value));

		let op = req!(value.get("op").and_then(|x| x.as_u64()));
		if op == 0 {
			Ok(GatewayEvent::Dispatch(
				req!(try!(remove(&mut value, "s")).as_u64()),
				try!(Event::decode(
					try!(remove(&mut value, "t").and_then(into_string)),
					try!(remove(&mut value, "d"))
				))
			))
		} else if op == 1 {
			Ok(GatewayEvent::Heartbeat(req!(try!(remove(&mut value, "s")).as_u64())))
		} else if op == 7 {
			Ok(GatewayEvent::Reconnect)
		} else if op == 9 {
			Ok(GatewayEvent::InvalidateSession)
		} else {
			Err(Error::Decode("Unexpected opcode", Value::Object(value)))
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

fn opt<T, F: FnOnce(Value) -> Result<T>>(map: &mut BTreeMap<String, Value>, key: &str, f: F) -> Result<Option<T>> {
	match map.remove(key) {
		None | Some(Value::Null) => Ok(None),
		Some(val) => f(val).map(Some),
	}
}

fn decode_discriminator(value: Value) -> Result<String> {
	match value {
		Value::String(s) => Ok(s),
		Value::I64(v) => Ok(v.to_string()),
		Value::U64(v) => Ok(v.to_string()),
		other => Err(Error::Decode("Expected string or u64", other))
	}
}

fn decode_notes(value: Value) -> Result<BTreeMap<UserId, String>> {
	// turn the String -> Value map into a UserId -> String map
	try!(into_map(value)).into_iter().map(|(key, value)| Ok((
		/* key */ UserId(try!(key.parse::<u64>().map_err(|_| Error::Other("Invalid user id in notes")))),
		/* val */ try!(into_string(value))
	))).collect()
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
