//! Builder types used for patches and other complex data structures.
//!
//! These types do not usually need to be imported, but the methods available
//! on them are very relevant to where they are used.

use serde_json::Value;

use model::*;
use Object;

macro_rules! builder {
	($(#[$attr:meta] $name:ident($inner:ty);)*) => {
		$(
			#[$attr]
			pub struct $name<'a>(&'a mut $inner);

			impl<'a> $name<'a> {
				#[doc(hidden)]
				#[inline(always)]
				pub fn __build<F: FnOnce($name) -> $name>(f: F) -> $inner where $inner: Default {
					Self::__apply(f, Default::default())
				}

				#[doc(hidden)]
				pub fn __apply<F: FnOnce($name) -> $name>(f: F, mut inp: $inner) -> $inner {
					f($name(&mut inp));
					inp
				}
			}
		)*
	}
}

builder! {
	/// Patch content for the `edit_server` call.
	EditServer(Object);

	/// Patch content for the `edit_channel` call.
	EditChannel(Object);

	/// Patch content for the `edit_member` call.
	EditMember(Object);

	/// Patch content for the `edit_profile` call.
	EditProfile(Object);

	/// Patch content for the `edit_user_profile` call.
	EditUserProfile(Object);

	/// Patch content for the `send_embed` call.
	EmbedBuilder(Object);

	/// Inner patch content for the `send_embed` call.
	EmbedFooterBuilder(Object);

	/// Inner patch content for the `send_embed` call.
	EmbedAuthorBuilder(Object);

	/// Inner patch content for the `send_embed` call.
	EmbedFieldsBuilder(Vec<Value>);
}

macro_rules! set {
	($self:ident, $key:expr, $($rest:tt)*) => {{
		$self.0.insert($key.into(), json!($($rest)*)); $self
	}}
}

impl<'a> EditServer<'a> {
	/// Edit the server's name.
	pub fn name(self, name: &str) -> Self {
		set!(self, "name", name)
	}

	/// Edit the server's voice region.
	pub fn region(self, region: &str) -> Self {
		set!(self, "region", region)
	}

	/// Edit the server's icon. Use `None` to remove the icon.
	pub fn icon(self, icon: Option<&str>) -> Self {
		set!(self, "icon", icon)
	}

	/// Edit the server's AFK channel. Use `None` to select no AFK channel.
	pub fn afk_channel(self, channel: Option<ChannelId>) -> Self {
		set!(self, "afk_channel_id", channel)
	}

	/// Edit the server's AFK timeout.
	pub fn afk_timeout(self, timeout: u64) -> Self {
		set!(self, "afk_timeout", timeout)
	}

	/// Transfer ownership of the server to a new owner.
	pub fn owner(self, owner: UserId) -> Self {
		set!(self, "owner_id", owner.0)
	}

	/// Edit the verification level of the server.
	pub fn verification_level(self, verification_level: VerificationLevel) -> Self {
		set!(self, "verification_level", verification_level.num())
	}

	/// Edit the server's splash. Use `None` to remove the splash.
	pub fn splash(self, splash: Option<&str>) -> Self {
		set!(self, "splash", splash)
	}
}

impl<'a> EditChannel<'a> {
	/// Edit the channel's name.
	pub fn name(self, name: &str) -> Self {
		set!(self, "name", name)
	}

	/// Edit the text channel's topic.
	pub fn topic(self, topic: &str) -> Self {
		set!(self, "topic", topic)
	}

	/// Edit the channel's position in the list.
	pub fn position(self, position: u64) -> Self {
		set!(self, "position", position)
	}

	/// Edit the voice channel's bitrate.
	pub fn bitrate(self, bitrate: u64) -> Self {
		set!(self, "bitrate", bitrate)
	}

	/// Edit the voice channel's user limit. Both `None` and `Some(0)` mean "unlimited".
	pub fn user_limit(self, user_limit: u64) -> Self {
		set!(self, "user_limit", user_limit)
	}
}

impl<'a> EditMember<'a> {
	/// Edit the member's nickname. Supply the empty string to remove a nickname.
	pub fn nickname(self, nick: &str) -> Self {
		set!(self, "nick", nick)
	}

	/// Edit whether the member is server-muted.
	pub fn mute(self, mute: bool) -> Self {
		set!(self, "mute", mute)
	}

	/// Edit whether the member is server-deafened.
	pub fn deaf(self, deafen: bool) -> Self {
		set!(self, "deafen", deafen)
	}

	/// Edit the member's assigned roles.
	pub fn roles(self, roles: &[RoleId]) -> Self {
		set!(self, "roles", roles)
	}

	/// Move the member to another voice channel.
	pub fn channel(self, channel: ChannelId) -> Self {
		set!(self, "channel_id", channel.0)
	}
}

impl<'a> EditProfile<'a> {
	/// Edit the user's username. Must be between 2 and 32 characters long.
	pub fn username(self, username: &str) -> Self {
		set!(self, "username", username)
	}

	/// Edit the user's avatar. Use `None` to remove the avatar.
	pub fn avatar(self, icon: Option<&str>) -> Self {
		set!(self, "avatar", icon)
	}
}

impl<'a> EditUserProfile<'a> {
	/// Provide the user's current password for authentication. Required if
	/// the email or password is being changed.
	pub fn password(self, password: &str) -> Self {
		set!(self, "password", password)
	}

	/// Edit the user's email address.
	pub fn email(self, email: &str) -> Self {
		set!(self, "email", email)
	}

	/// Edit the user's password.
	pub fn new_password(self, password: &str) -> Self {
		set!(self, "new_password", password)
	}

	/// Edit the user's username. Must be between 2 and 32 characters long.
	pub fn username(self, username: &str) -> Self {
		set!(self, "username", username)
	}

	/// Edit the user's avatar. Use `None` to remove the avatar.
	pub fn avatar(self, icon: Option<&str>) -> Self {
		set!(self, "avatar", icon)
	}
}

impl<'a> EmbedBuilder<'a> {
	/// Add the "title of embed".
	pub fn title(self, title: &str) -> Self {
		set!(self, "title", title)
	}

	/// Add the "description of embed".
	pub fn description(self, description: &str) -> Self {
		set!(self, "description", description)
	}

	/// Add the "url of embed".
	pub fn url(self, url: &str) -> Self {
		set!(self, "url", url)
	}

	/// Add the "timestamp of embed content".
	pub fn timestamp(self, timestamp: &str) -> Self {
		set!(self, "timestamp", timestamp)
	}

	/// Add the "color code of the embed".
	pub fn color(self, color: u64) -> Self {
		set!(self, "color", color)
	}

	/// Add "footer information". See the `EmbedFooterBuilder` struct for the editable fields.
	pub fn footer<F: FnOnce(EmbedFooterBuilder) -> EmbedFooterBuilder>(self, f: F) -> Self {
		set!(self, "footer", EmbedFooterBuilder::__build(f))
	}

	/// Add "source url of image". Only supports http(s).
	pub fn image(self, url: &str) -> Self {
		set!(self, "image", { "url": url })
	}

	/// Add "source url of thumbnail". Only supports http(s).
	pub fn thumbnail(self, url: &str) -> Self {
		set!(self, "thumbnail", { "url": url })
	}

	/// Add "author information". See the `EmbedAuthorBuilder` struct for the editable fields.
	pub fn author<F: FnOnce(EmbedAuthorBuilder) -> EmbedAuthorBuilder>(self, f: F) -> Self {
		set!(self, "author", EmbedAuthorBuilder::__build(f))
	}

	/// Add "fields information". See the `EmbedFieldsBuilder` struct for the editable fields.
	pub fn fields<F: FnOnce(EmbedFieldsBuilder) -> EmbedFieldsBuilder>(self, f: F) -> Self {
		set!(self, "fields", EmbedFieldsBuilder::__build(f))
	}
}

impl<'a> EmbedFooterBuilder<'a> {
	/// Add the "footer text".
	pub fn text(self, text: &str) -> Self {
		set!(self, "text", text)
	}

	/// Add the "url of footer icon". Only the http(s) protocols are supported.
	pub fn icon_url(self, icon_url: &str) -> Self {
		set!(self, "icon_url", icon_url)
	}
}

impl<'a> EmbedAuthorBuilder<'a> {
	/// Add the "name of author".
	pub fn name(self, name: &str) -> Self {
		set!(self, "name", name)
	}

	/// Add the "url of author".
	pub fn url(self, url: &str) -> Self {
		set!(self, "url", url)
	}

	/// Add the "url of author icon". Only the http(s) protocols are supported.
	pub fn icon_url(self, icon_url: &str) -> Self {
		set!(self, "icon_url", icon_url)
	}
}

impl<'a> EmbedFieldsBuilder<'a> {
	/// Add an entire field structure, representing a mapping from `name` to `value`.
	///
	/// `inline` determines "whether or not this field should display inline".
	pub fn field(self, name: &str, value: &str, inline: bool) -> Self {
		self.0.push(json! {{
			"name": name,
			"value": value,
			"inline": inline,
		}});
		self
	}
}
