//! Builder types used for patches and other complex data structures.
//!
//! These types do not usually need to be imported, but the methods available
//! on them are very relevant to where they are used.

use serde_json::builder::{ObjectBuilder, ArrayBuilder};
use model::*;

macro_rules! builder {
	($(#[$attr:meta] $name:ident($inner:ty);)*) => {
		$(
			#[$attr]
			pub struct $name($inner);

			impl $name {
				#[doc(hidden)]
				pub fn __build<F: FnOnce($name) -> $name>(f: F, inp: $inner) -> $inner {
					f($name(inp)).0
				}
			}
		)*
	}
}

builder! {
	/// Patch content for the `edit_server` call.
	EditServer(ObjectBuilder);

	/// Patch content for the `edit_channel` call.
	EditChannel(ObjectBuilder);

	/// Patch content for the `edit_member` call.
	EditMember(ObjectBuilder);

	/// Patch content for the `edit_profile` call.
	EditProfile(ObjectBuilder);

	/// Patch content for the `edit_user_profile` call.
	EditUserProfile(ObjectBuilder);

	/// Patch content for the `send_embed` call.
	EmbedBuilder(ObjectBuilder);

	/// Inner patch content for the `send_embed` call.
	EmbedFooterBuilder(ObjectBuilder);

	/// Inner patch content for the `send_embed` call.
	EmbedAuthorBuilder(ObjectBuilder);

	/// Inner patch content for the `send_embed` call.
	EmbedFieldsBuilder(ArrayBuilder);
}

impl EditServer {
	/// Edit the server's name.
	pub fn name(self, name: &str) -> Self {
		EditServer(self.0.insert("name", name))
	}

	/// Edit the server's voice region.
	pub fn region(self, region: &str) -> Self {
		EditServer(self.0.insert("region", region))
	}

	/// Edit the server's icon. Use `None` to remove the icon.
	pub fn icon(self, icon: Option<&str>) -> Self {
		EditServer(self.0.insert("icon", icon))
	}

	/// Edit the server's AFK channel. Use `None` to select no AFK channel.
	pub fn afk_channel(self, channel: Option<ChannelId>) -> Self {
		EditServer(self.0.insert("afk_channel_id", channel.map(|c| c.0)))
	}

	/// Edit the server's AFK timeout.
	pub fn afk_timeout(self, timeout: u64) -> Self {
		EditServer(self.0.insert("afk_timeout", timeout))
	}

	/// Transfer ownership of the server to a new owner.
	pub fn owner(self, owner: UserId) -> Self {
		EditServer(self.0.insert("owner_id", owner.0))
	}

	/// Edit the verification level of the server.
	pub fn verification_level(self, verification_level: VerificationLevel) -> Self {
		EditServer(self.0.insert("verification_level", verification_level.num()))
	}

	/// Edit the server's splash. Use `None` to remove the splash.
	pub fn splash(self, splash: Option<&str>) -> Self {
		EditServer(self.0.insert("splash", splash))
	}
}

impl EditChannel {
	/// Edit the channel's name.
	pub fn name(self, name: &str) -> Self {
		EditChannel(self.0.insert("name", name))
	}

	/// Edit the text channel's topic.
	pub fn topic(self, topic: &str) -> Self {
		EditChannel(self.0.insert("topic", topic))
	}

	/// Edit the channel's position in the list.
	pub fn position(self, position: u64) -> Self {
		EditChannel(self.0.insert("position", position))
	}

	/// Edit the voice channel's bitrate.
	pub fn bitrate(self, bitrate: u64) -> Self {
		EditChannel(self.0.insert("bitrate", bitrate))
	}

	/// Edit the voice channel's user limit. Both `None` and `Some(0)` mean "unlimited".
	pub fn user_limit(self, user_limit: u64) -> Self {
		EditChannel(self.0.insert("user_limit", user_limit))
	}
}

impl EditMember {
	/// Edit the member's nickname. Supply the empty string to remove a nickname.
	pub fn nickname(self, nick: &str) -> Self {
		EditMember(self.0.insert("nick", nick))
	}

	/// Edit whether the member is server-muted.
	pub fn mute(self, mute: bool) -> Self {
		EditMember(self.0.insert("mute", mute))
	}

	/// Edit whether the member is server-deafened.
	pub fn deaf(self, deafen: bool) -> Self {
		EditMember(self.0.insert("deaf", deafen))
	}

	/// Edit the member's assigned roles.
	pub fn roles(self, roles: &[RoleId]) -> Self {
		EditMember(self.0.insert_array("roles",
			|ab| roles.iter().fold(ab, |ab, id| ab.push(id.0))))
	}

	/// Move the member to another voice channel.
	pub fn channel(self, channel: ChannelId) -> Self {
		EditMember(self.0.insert("channel_id", channel.0))
	}
}

impl EditProfile {
	/// Edit the user's username. Must be between 2 and 32 characters long.
	pub fn username(self, username: &str) -> Self {
		EditProfile(self.0.insert("username", username))
	}

	/// Edit the user's avatar. Use `None` to remove the avatar.
	pub fn avatar(self, icon: Option<&str>) -> Self {
		EditProfile(self.0.insert("avatar", icon))
	}
}

impl EditUserProfile {
	/// Provide the user's current password for authentication. Required if
	/// the email or password is being changed.
	pub fn password(self, password: &str) -> Self {
		EditUserProfile(self.0.insert("password", password))
	}

	/// Edit the user's email address.
	pub fn email(self, email: &str) -> Self {
		EditUserProfile(self.0.insert("email", email))
	}

	/// Edit the user's password.
	pub fn new_password(self, password: &str) -> Self {
		EditUserProfile(self.0.insert("new_password", password))
	}

	/// Edit the user's username. Must be between 2 and 32 characters long.
	pub fn username(self, username: &str) -> Self {
		EditUserProfile(self.0.insert("username", username))
	}

	/// Edit the user's avatar. Use `None` to remove the avatar.
	pub fn avatar(self, icon: Option<&str>) -> Self {
		EditUserProfile(self.0.insert("avatar", icon))
	}
}

impl EmbedBuilder {
	/// Add the "title of embed".
	pub fn title(self, title: &str) -> Self {
		EmbedBuilder(self.0.insert("title", title))
	}

	/// Add the "description of embed".
	pub fn description(self, description: &str) -> Self {
		EmbedBuilder(self.0.insert("description", description))
	}

	/// Add the "url of embed".
	pub fn url(self, url: &str) -> Self {
		EmbedBuilder(self.0.insert("url", url))
	}

	/// Add the "timestamp of embed content".
	pub fn timestamp(self, timestamp: &str) -> Self {
		EmbedBuilder(self.0.insert("timestamp", timestamp))
	}

	/// Add the "color code of the embed".
	pub fn color(self, color: u64) -> Self {
		EmbedBuilder(self.0.insert("color", color))
	}

	/// Add "footer information". See the `EmbedFooterBuilder` struct for the editable fields.
	pub fn footer<F: FnOnce(EmbedFooterBuilder) -> EmbedFooterBuilder>(self, f: F) -> Self {
		EmbedBuilder(self.0.insert("footer", f(EmbedFooterBuilder(ObjectBuilder::new())).0.build()))
	}

	/// Add "source url of image". Only supports http(s).
	pub fn image(self, url: &str) -> Self {
		EmbedBuilder(self.0.insert("image", ObjectBuilder::new().insert("url", url).build()))
	}

	/// Add "source url of thumbnail". Only supports http(s).
	pub fn thumbnail(self, url: &str) -> Self {
		EmbedBuilder(self.0.insert("thumbnail", ObjectBuilder::new().insert("url", url).build()))
	}

	/// Add "author information". See the `EmbedAuthorBuilder` struct for the editable fields.
	pub fn author<F: FnOnce(EmbedAuthorBuilder) -> EmbedAuthorBuilder>(self, f: F) -> Self {
		EmbedBuilder(self.0.insert("author", f(EmbedAuthorBuilder(ObjectBuilder::new())).0.build()))
	}

	/// Add "fields information". See the `EmbedFieldsBuilder` struct for the editable fields.
	pub fn fields<F: FnOnce(EmbedFieldsBuilder) -> EmbedFieldsBuilder>(self, f: F) -> Self {
		EmbedBuilder(self.0.insert("fields", f(EmbedFieldsBuilder(ArrayBuilder::new())).0.build()))
	}
}

impl EmbedFooterBuilder {
	/// Add the "footer text".
	pub fn text(self, text: &str) -> Self {
		EmbedFooterBuilder(self.0.insert("text", text))
	}

	/// Add the "url of footer icon". Only the http(s) protocols are supported.
	pub fn icon_url(self, icon_url: &str) -> Self {
		EmbedFooterBuilder(self.0.insert("icon_url", icon_url))
	}
}

impl EmbedAuthorBuilder {
	/// Add the "name of author".
	pub fn name(self, name: &str) -> Self {
		EmbedAuthorBuilder(self.0.insert("name", name))
	}

	/// Add the "url of author".
	pub fn url(self, url: &str) -> Self {
		EmbedAuthorBuilder(self.0.insert("url", url))
	}

	/// Add the "url of author icon". Only the http(s) protocols are supported.
	pub fn icon_url(self, icon_url: &str) -> Self {
		EmbedAuthorBuilder(self.0.insert("icon_url", icon_url))
	}
}

impl EmbedFieldsBuilder {
	/// Add an entire field structure, representing a mapping from `name` to `value`.
	///
	/// `inline` determines "whether or not this field should display inline".
	pub fn field(self, name: &str, value: &str, inline: bool) -> Self {
		EmbedFieldsBuilder(self.0.push(ObjectBuilder::new().insert("name", name).insert("value", value).insert("inline", inline).build()))
	}
}
