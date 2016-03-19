//! Client library for the [Discord](https://discordapp.com) API.
//!
//! The Discord API can be divided into three main components: the RESTful API
//! to which calls can be made to take actions, a websocket-based permanent
//! connection over which state updates are received, and the voice calling
//! system.
//!
//! Log in to Discord with `Discord::new`. The resulting value can be used to
//! make REST API calls to post messages and manipulate Discord state. Calling
//! `connect()` will open a websocket connection, through which events can be
//! received. These two channels are enough to write a simple chatbot which can
//! read and respond to messages.
//!
//! For more in-depth tracking of Discord state, a `State` can be seeded with
//! the `ReadyEvent` obtained when opening a `Connection` and kept updated with
//! the events received over it.
//!
//! To use the voice call system, initialize a `VoiceConnection` with the user id
//! received in the `ReadyEvent`, call `voice_connect` on the `Connection`, and
//! pass events to `VoiceConnection::update`. Once the connection has been
//! established, the `play` and `stop` methods can be used to control playback.
//!
//! For examples, see the `examples` directory in the source tree.
#![warn(missing_docs)]

extern crate hyper;
extern crate serde_json;
extern crate websocket;
#[macro_use]
extern crate bitflags;
extern crate byteorder;
extern crate opus;
extern crate time;
#[macro_use]
extern crate log;
extern crate sodiumoxide;
extern crate multipart;
extern crate base64;

use std::collections::BTreeMap;
use serde_json::builder::ObjectBuilder;

mod error;
mod connection;
mod state;
pub mod voice;
pub mod model;

pub use error::{Result, Error};
pub use connection::Connection;
pub use state::{State, ChannelRef};
use model::*;

const USER_AGENT: &'static str = concat!("DiscordBot (https://github.com/SpaceManiac/discord-rs, ", env!("CARGO_PKG_VERSION"), ")");
const API_BASE: &'static str = "https://discordapp.com/api";
const GATEWAY_VERSION: u64 = 3;

/// Client for the Discord REST API.
///
/// Log in to the API with a user's email and password using `new()`. Call
/// `connect()` to create a `Connection` on which to receive events. If desired,
/// use `logout()` to invalidate the token when done. Other methods manipulate
/// the Discord REST API.
pub struct Discord {
	client: hyper::Client,
	token: String,
}

impl Discord {
	/// Log in to the Discord Rest API and acquire a token.
	pub fn new(email: &str, password: &str) -> Result<Discord> {
		let mut map = BTreeMap::new();
		map.insert("email", email);
		map.insert("password", password);

		let client = hyper::Client::new();
		let response = try!(check_status(client.post(&format!("{}/auth/login", API_BASE))
			.header(hyper::header::ContentType::json())
			.header(hyper::header::UserAgent(USER_AGENT.to_owned()))
			.body(&try!(serde_json::to_string(&map)))
			.send()));
		let mut json: BTreeMap<String, String> = try!(serde_json::from_reader(response));
		let token = match json.remove("token") {
			Some(token) => token,
			None => return Err(Error::Protocol("Response missing \"token\" in Discord::new()"))
		};
		Ok(Discord {
			client: client,
			token: token,
		})
	}

	/// Log in to the Discord Rest API, possibly using a cached login token.
	///
	/// Cached login tokens are keyed to the email address and will be read from
	/// and written to the specified path. If no cached token was found and no
	/// password was specified, an error is returned.
	pub fn new_cache<P: AsRef<std::path::Path>>(path: P, email: &str, password: Option<&str>) -> Result<Discord> {
		use std::io::{Write, BufRead, BufReader};
		use std::fs::File;

		// Read the cache, looking for our token
		let path = path.as_ref();
		let mut initial_token: Option<String> = None;
		if let Ok(file) = File::open(path) {
			for line in BufReader::new(file).lines() {
				let line = try!(line);
				let parts: Vec<_> = line.split('\t').collect();
				if parts.len() == 2 && parts[0] == email {
					initial_token = Some(parts[1].trim().into());
					break;
				}
			}
		}

		// Perform the login
		let discord = if let Some(ref initial_token) = initial_token {
			let mut map = BTreeMap::new();
			map.insert("email", email);
			if let Some(password) = password {
				map.insert("password", password);
			}

			let client = hyper::Client::new();
			let response = try!(check_status(client.post(&format!("{}/auth/login", API_BASE))
				.header(hyper::header::ContentType::json())
				.header(hyper::header::UserAgent(USER_AGENT.to_owned()))
				.header(hyper::header::Authorization(initial_token.clone()))
				.body(&try!(serde_json::to_string(&map)))
				.send()));
			let mut json: BTreeMap<String, String> = try!(serde_json::from_reader(response));
			let token = match json.remove("token") {
				Some(token) => token,
				None => return Err(Error::Protocol("Response missing \"token\" in Discord::new()"))
			};
			Discord {
				client: client,
				token: token,
			}
		} else {
			if let Some(password) = password {
				try!(Discord::new(email, password))
			} else {
				return Err(Error::Other("No password was specified and no cached token was found"))
			}
		};

		// Write the token back out, if needed
		if initial_token.as_ref() != Some(&discord.token) {
			let mut tokens = Vec::new();
			tokens.push(format!("{}\t{}", email, discord.token));
			if let Ok(file) = File::open(path) {
				for line in BufReader::new(file).lines() {
					let line = try!(line);
					if line.split('\t').next() != Some(email) {
						tokens.push(line);
					}
				}
			}
			let mut file = try!(File::create(path));
			for line in tokens {
				try!(file.write_all(line.as_bytes()));
				try!(file.write_all(&[b'\n']));
			}
		}

		Ok(discord)
	}

	/// Log in as a bot account using the given authentication token.
	pub fn from_bot_token(token: &str) -> Result<Discord> {
		Ok(Discord {
			client: hyper::Client::new(),
			// TODO: use "Bot {}" when the gateway bug is fixed
			token: format!("{}", token),
		})
	}

	/// Log out from the Discord API, invalidating this clients's token.
	pub fn logout(self) -> Result<()> {
		let map = ObjectBuilder::new().insert("token", &self.token).unwrap();
		let body = try!(serde_json::to_string(&map));
		try!(retry(|| self.client.post(&format!("{}/auth/logout", API_BASE))
			.header(hyper::header::ContentType::json())
			.body(&body)));
		Ok(())
	}

	fn request<'a, F: Fn() -> hyper::client::RequestBuilder<'a>>(&self, f: F) -> Result<hyper::client::Response> {
		retry(|| f()
			.header(hyper::header::ContentType::json())
			.header(hyper::header::Authorization(self.token.clone())))
	}

	/// Create a channel.
	pub fn create_channel(&self, server: &ServerId, name: &str, kind: ChannelType) -> Result<Channel> {
		let map = ObjectBuilder::new()
			.insert("name", name)
			.insert("type", kind.name())
			.unwrap();
		let body = try!(serde_json::to_string(&map));
		let response = try!(self.request(||
			self.client.post(&format!("{}/guilds/{}/channels", API_BASE, server.0)).body(&body)));
		Channel::decode(try!(serde_json::from_reader(response)))
	}

	/// Edit a channel's name.
	pub fn edit_channel(&self, channel: &ChannelId,
		name: Option<&str>,
		position: Option<i64>,
		topic: Option<&str>,
	) -> Result<Channel> {
		let map = ObjectBuilder::new()
			.insert("name", name)
			.insert("topic", topic)
			.insert("position", position)
			.unwrap();
		let body = try!(serde_json::to_string(&map));
		let response = try!(self.request(||
			self.client.patch(&format!("{}/channels/{}", API_BASE, channel.0)).body(&body)));
		Channel::decode(try!(serde_json::from_reader(response)))
	}

	/// Delete a channel.
	pub fn delete_channel(&self, channel: &ChannelId) -> Result<Channel> {
		let response = try!(self.request(||
			self.client.delete(&format!("{}/channels/{}", API_BASE, channel.0))));
		Channel::decode(try!(serde_json::from_reader(response)))
	}

	/// Indicate typing on a channel for the next 5 seconds.
	pub fn broadcast_typing(&self, channel: &ChannelId) -> Result<()> {
		try!(self.request(|| self.client.post(&format!("{}/channels/{}/typing", API_BASE, channel.0))));
		Ok(())
	}

	/// Get messages in the backlog for a given channel.
	///
	/// Before and after limits can be specified to narrow the results. A
	/// message count limit can also be specified, defaulting to 50. Newer
	/// messages appear first in the list.
	pub fn get_messages(&self, channel: &ChannelId, before: Option<&MessageId>, after: Option<&MessageId>, limit: Option<u64>) -> Result<Vec<Message>> {
		let mut url = format!("{}/channels/{}/messages?limit={}", API_BASE, channel.0, limit.unwrap_or(50));
		if let Some(id) = before {
			url.push_str(&format!("&before={}", id.0));
		}
		if let Some(id) = after {
			url.push_str(&format!("&after={}", id.0));
		}
		let response = try!(self.request(|| self.client.get(&url)));
		let values: Vec<serde_json::Value> = try!(serde_json::from_reader(response));
		values.into_iter().map(Message::decode).collect()
	}

	/// Send a message to a given channel.
	///
	/// The `nonce` will be returned in the result and also transmitted to other
	/// clients. The empty string is a good default if you don't care.
	pub fn send_message(&self, channel: &ChannelId, text: &str, nonce: &str, tts: bool) -> Result<Message> {
		let map = ObjectBuilder::new()
			.insert("content", text)
			.insert("nonce", nonce)
			.insert("tts", tts)
			.unwrap();
		let body = try!(serde_json::to_string(&map));
		let response = try!(self.request(||
			self.client.post(&format!("{}/channels/{}/messages", API_BASE, channel.0)).body(&body)));
		Message::decode(try!(serde_json::from_reader(response)))
	}

	/// Edit a previously posted message.
	///
	/// Requires that either the message was posted by this user, or this user
	/// has permission to manage other members' messages.
	pub fn edit_message(&self, channel: &ChannelId, message: &MessageId, text: &str) -> Result<Message> {
		let map = ObjectBuilder::new()
			.insert("content", text)
			.unwrap();
		let body = try!(serde_json::to_string(&map));
		let response = try!(self.request(||
			self.client.patch(&format!("{}/channels/{}/messages/{}", API_BASE, channel.0, message.0)).body(&body)));
		Message::decode(try!(serde_json::from_reader(response)))
	}

	/// Delete a previously posted message.
	///
	/// Requires that either the message was posted by this user, or this user
	/// has permission to manage other members' messages.
	pub fn delete_message(&self, channel: &ChannelId, message: &MessageId) -> Result<()> {
		try!(self.request(||
			self.client.delete(&format!("{}/channels/{}/messages/{}", API_BASE, channel.0, message.0))));
		Ok(())
	}

	/// Send a file attached to a message on a given channel.
	///
	/// The `text` is allowed to be empty, but the filename must always be specified.
	pub fn send_file<R: ::std::io::Read>(&self, channel: &ChannelId, text: &str, mut file: R, filename: &str) -> Result<Message> {
		let url = match hyper::Url::parse(&format!("{}/channels/{}/messages", API_BASE, channel.0)) {
			Ok(url) => url,
			Err(_) => return Err(Error::Other("Invalid URL in send_file"))
		};
		let mut request = try!(hyper::client::Request::new(hyper::method::Method::Post, url));
		request.headers_mut().set(hyper::header::Authorization(self.token.clone()));
		let mut request = try!(multipart::client::Multipart::from_request(request));
		try!(request.write_text("content", text));
		try!(request.write_stream("file", &mut file, Some(filename), None));
		Message::decode(try!(serde_json::from_reader(try!(request.send()))))
	}

	/// Acknowledge this message as "read" by this client.
	pub fn ack_message(&self, channel: &ChannelId, message: &MessageId) -> Result<()> {
		try!(self.request(||
			self.client.post(&format!("{}/channels/{}/messages/{}/ack", API_BASE, channel.0, message.0))));
		Ok(())
	}

	// TODO: the remaining API calls
	/*
	pub fn create_role_permission(&self, channel: &ChannelId, role: &RoleId, allow: Permissions, deny: Permissions, type: Role|Member)
	pub fn delete_permission(&self, channel: &ChannelId, role: &RoleId);
	*/

	/// Get the list of servers this user knows about.
	pub fn get_servers(&self) -> Result<Vec<ServerInfo>> {
		let response = try!(self.request(||
			self.client.get(&format!("{}/users/@me/guilds", API_BASE))));
		decode_array(try!(serde_json::from_reader(response)), ServerInfo::decode)
	}

	/// Create a new server with the given name.
	pub fn create_server(&self, name: &str, region: &str, icon: Option<&str>) -> Result<Server> {
		let map = ObjectBuilder::new()
			.insert("name", name)
			.insert("region", region)
			.insert("icon", icon)
			.unwrap();
		let body = try!(serde_json::to_string(&map));
		let response = try!(self.request(||
			self.client.post(&format!("{}/guilds", API_BASE)).body(&body)));
		Server::decode(try!(serde_json::from_reader(response)))
	}

	/// Edit a server's information. See `EditServer` for the editable fields.
	///
	/// ```ignore
	/// // Rename a server
	/// discord.edit_server(server_id, |server| server.name("My Cool Server"));
	/// // Edit many properties at once
	/// discord.edit_server(server_id, |server| server
	///     .name("My Cool Server")
	///     .icon(Some("data:image/jpg;base64,..."))
	///     .afk_timeout(300)
	///     .region("us-south")
	/// );
	/// ```
	pub fn edit_server<F: FnOnce(EditServer) -> EditServer>(&self, server_id: ServerId, f: F) -> Result<Server> {
		let map = f(EditServer(ObjectBuilder::new())).0.unwrap();
		let body = try!(serde_json::to_string(&map));
		let response = try!(self.request(||
			self.client.patch(&format!("{}/guilds/{}", API_BASE, server_id.0)).body(&body)));
		Server::decode(try!(serde_json::from_reader(response)))
	}

	/// Leave the given server.
	pub fn leave_server(&self, server: &ServerId) -> Result<Server> {
		let response = try!(self.request(||
			self.client.delete(&format!("{}/users/@me/guilds/{}", API_BASE, server.0))));
		Server::decode(try!(serde_json::from_reader(response)))
	}

	/// Delete the given server. Only available to the server owner.
	pub fn delete_server(&self, server: &ServerId) -> Result<Server> {
		let response = try!(self.request(||
			self.client.delete(&format!("{}/guilds/{}", API_BASE, server.0))));
		Server::decode(try!(serde_json::from_reader(response)))
	}

	/// Get the ban list for the given server.
	pub fn get_bans(&self, server: &ServerId) -> Result<Vec<User>> {
		let response = try!(self.request(||
			self.client.get(&format!("{}/guilds/{}/bans", API_BASE, server.0))));
		decode_array(try!(serde_json::from_reader(response)), User::decode_ban)
	}

	/// Ban a user from the server, optionally deleting their recent messages.
	///
	/// Zero may be passed for `delete_message_days` if no deletion is desired.
	pub fn add_ban(&self, server: &ServerId, user: &UserId, delete_message_days: u32) -> Result<()> {
		try!(self.request(|| self.client.put(
			&format!("{}/guilds/{}/bans/{}?delete_message_days={}", API_BASE, server.0, user.0, delete_message_days))));
		Ok(())
	}

	/// Unban a user from the server.
	pub fn remove_ban(&self, server: &ServerId, user: &UserId) -> Result<()> {
		try!(self.request(|| self.client.delete(
			&format!("{}/guilds/{}/bans/{}", API_BASE, server.0, user.0))));
		Ok(())
	}

	/// Extract information from an invite.
	///
	/// The invite should either be a URL of the form `http://discord.gg/CODE`,
	/// or a string containing just the `CODE`.
	pub fn get_invite(&self, invite: &str) -> Result<Invite> {
		let invite = resolve_invite(invite);
		let response = try!(self.request(||
			self.client.get(&format!("{}/invite/{}", API_BASE, invite))));
		Invite::decode(try!(serde_json::from_reader(response)))
	}

	/// Get the active invites for a server.
	pub fn get_server_invites(&self, server: ServerId) -> Result<Vec<RichInvite>> {
		let response = try!(self.request(||
			self.client.get(&format!("{}/guilds/{}/invites", API_BASE, server.0))));
		decode_array(try!(serde_json::from_reader(response)), RichInvite::decode)
	}

	/// Get the active invites for a channel.
	pub fn get_channel_invites(&self, channel: ChannelId) -> Result<Vec<RichInvite>> {
		let response = try!(self.request(||
			self.client.get(&format!("{}/channels/{}/invites", API_BASE, channel.0))));
		decode_array(try!(serde_json::from_reader(response)), RichInvite::decode)
	}

	/// Accept an invite. See `get_invite` for details.
	pub fn accept_invite(&self, invite: &str) -> Result<Invite> {
		let invite = resolve_invite(invite);
		let response = try!(self.request(||
			self.client.post(&format!("{}/invite/{}", API_BASE, invite))));
		Invite::decode(try!(serde_json::from_reader(response)))
	}

	/// Create an invite to a channel.
	///
	/// Passing 0 for `max_age` or `max_uses` means no limit. `max_age` should be specified in
	/// seconds. Enabling `xkcdpass` forces a 30-minute expiry.
	pub fn create_invite(&self, channel: ChannelId,
		max_age: u64, max_uses: u64,
		temporary: bool, xkcdpass: bool
	) -> Result<RichInvite> {
		let map = ObjectBuilder::new()
			.insert("validate", serde_json::Value::Null)
			.insert("max_age", max_age)
			.insert("max_uses", max_uses)
			.insert("temporary", temporary)
			.insert("xkcdpass", xkcdpass)
			.unwrap();
		let body = try!(serde_json::to_string(&map));
		let response = try!(self.request(||
			self.client.post(&format!("{}/channels/{}/invites", API_BASE, channel.0)).body(&body)));
		RichInvite::decode(try!(serde_json::from_reader(response)))
	}

	/// Delete an invite. See `get_invite` for details.
	pub fn delete_invite(&self, invite: &str) -> Result<Invite> {
		let invite = resolve_invite(invite);
		let response = try!(self.request(||
			self.client.delete(&format!("{}/invite/{}", API_BASE, invite))));
		Invite::decode(try!(serde_json::from_reader(response)))
	}

	// Get members

	/// Edit the list of roles assigned to a member of a server.
	pub fn edit_member_roles(&self, server: &ServerId, user: &UserId, roles: &[&RoleId]) -> Result<()> {
		let map = ObjectBuilder::new()
			.insert_array("roles", |ab| roles.iter().fold(ab, |ab, id| ab.push(id.0)))
			.unwrap();
		let body = try!(serde_json::to_string(&map));
		try!(self.request(|| self.client.patch(
			&format!("{}/guilds/{}/members/{}", API_BASE, server.0, user.0)).body(&body)));
		Ok(())
	}

	/// Kick a member from a server.
	pub fn kick_member(&self, server: &ServerId, user: &UserId) -> Result<()> {
		try!(self.request(|| self.client.delete(
			&format!("{}/guilds/{}/members/{}", API_BASE, server.0, user.0))));
		Ok(())
	}

	// Create role
	// Edit role
	// Reorder roles
	// Delete roles

	/// Create a private channel with the given user, or return the existing
	/// one if it exists.
	pub fn create_private_channel(&self, recipient: &UserId) -> Result<PrivateChannel> {
		let map = ObjectBuilder::new()
			.insert("recipient_id", &recipient.0)
			.unwrap();
		let body = try!(serde_json::to_string(&map));
		let response = try!(self.request(||
			self.client.post(&format!("{}/users/@me/channels", API_BASE)).body(&body)));
		PrivateChannel::decode(try!(serde_json::from_reader(response)))
	}

	/// Get the URL at which a user's avatar is located.
	pub fn get_user_avatar_url(&self, user: &UserId, avatar: &str) -> String {
		format!("{}/users/{}/avatars/{}.jpg", API_BASE, user.0, avatar)
	}

	/// Download a user's avatar.
	pub fn get_user_avatar(&self, user: &UserId, avatar: &str) -> Result<Vec<u8>> {
		use std::io::Read;
		let mut response = try!(retry(||
			self.client.get(&self.get_user_avatar_url(user, avatar))));
		let mut vec = Vec::new();
		try!(response.read_to_end(&mut vec));
		Ok(vec)
	}

	/// Edit the logged-in user's profile. See `EditProfile` for editable fields.
	pub fn edit_profile<F: FnOnce(EditProfile) -> EditProfile>(&mut self, f: F) -> Result<CurrentUser> {
		// First, get the current profile, so that providing username and avatar is optional.
		let response = try!(self.request(||
			self.client.get(&format!("{}/users/@me", API_BASE))));
		let user = try!(CurrentUser::decode(try!(serde_json::from_reader(response))));
		let mut map = ObjectBuilder::new()
			.insert("username", user.username)
			.insert("avatar", user.avatar);
		if let Some(email) = user.email.as_ref() {
			map = map.insert("email", email);
		}

		// Then, send the profile patch.
		let map = f(EditProfile(map)).0.unwrap();
		let body = try!(serde_json::to_string(&map));
		let response = try!(self.request(||
			self.client.patch(&format!("{}/users/@me", API_BASE)).body(&body)));
		let mut json: BTreeMap<String, serde_json::Value> = try!(serde_json::from_reader(response));
		// If a token was included in the response, switch to it. Important because if the
		// password was changed, the old token is invalidated.
		if let Some(serde_json::Value::String(token)) = json.remove("token") {
			self.token = token;
		}
		CurrentUser::decode(serde_json::Value::Object(json))
	}

	// Get active maintenances
	// Get upcoming maintenances

	/// Get the list of available voice regions for a server.
	pub fn get_voice_regions(&self) -> Result<Vec<VoiceRegion>> {
		let response = try!(self.request(|| self.client.get(&format!("{}/voice/regions", API_BASE))));
		decode_array(try!(serde_json::from_reader(response)), VoiceRegion::decode)
	}

	/// Move a server member to another voice channel.
	pub fn move_member_voice(&self, server: &ServerId, user: &UserId, channel: &ChannelId) -> Result<()> {
		let map = ObjectBuilder::new()
			.insert("channel_id", &channel.0)
			.unwrap();
		let body = try!(serde_json::to_string(&map));
		try!(self.request(||
			self.client.patch(&format!("{}/guilds/{}/members/{}", API_BASE, server.0, user.0)).body(&body)));
		Ok(())
	}

	/// Establish a websocket connection over which events can be received.
	///
	/// Also returns the `ReadyEvent` sent by Discord upon establishing the
	/// connection, which contains the initial state as seen by the client.
	pub fn connect(&self) -> Result<(Connection, ReadyEvent)> {
		let response = try!(self.request(|| self.client.get(&format!("{}/gateway", API_BASE))));
		let value: BTreeMap<String, String> = try!(serde_json::from_reader(response));
		let url = match value.get("url") {
			Some(url) => url,
			None => return Err(Error::Protocol("Response missing \"url\" in Discord::connect()"))
		};
		Connection::new(&url, &self.token)
	}
}

/// Read an image from a file into a string suitable for upload.
pub fn read_image<P: AsRef<::std::path::Path>>(path: P) -> Result<String> {
	use std::io::Read;
	let path = path.as_ref();
	let mut vec = Vec::new();
	try!(try!(std::fs::File::open(path)).read_to_end(&mut vec));
	Ok(format!("data:image/{};base64,{}",
		if path.extension() == Some("png".as_ref()) { "png" } else { "jpg" },
		base64::encode(&vec),
	))
}

/// Patch content for the `edit_server` call.
pub struct EditServer(ObjectBuilder);

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
		EditServer(match icon {
			Some(icon) => self.0.insert("icon", icon),
			None => self.0.insert("icon", serde_json::Value::Null),
		})
	}
	/// Edit the server's AFK channel. Use `None` to select no AFK channel.
	pub fn afk_channel(self, channel: Option<ChannelId>) -> Self {
		EditServer(match channel {
			Some(ch) => self.0.insert("afk_channel_id", ch.0),
			None => self.0.insert("afk_channel_id", serde_json::Value::Null),
		})
	}
	/// Edit the server's AFK timeout.
	pub fn afk_timeout(self, timeout: u64) -> Self {
		EditServer(self.0.insert("afk_timeout", timeout))
	}
}

/// Patch content for the `edit_profile` call.
pub struct EditProfile(ObjectBuilder);

impl EditProfile {
	/// Edit the user's username. Must be between 2 and 32 characters long.
	pub fn username(self, username: &str) -> Self {
		EditProfile(self.0.insert("username", username))
	}
	/// Edit the user's avatar. Use `None` to remove the avatar.
	pub fn avatar(self, icon: Option<&str>) -> Self {
		EditProfile(match icon {
			Some(icon) => self.0.insert("avatar", icon),
			None => self.0.insert("avatar", serde_json::Value::Null),
		})
	}
	/// Provide the user's current password for authentication. Does not apply to bot accounts, and
	/// must be supplied for user accounts.
	pub fn password(self, password: &str) -> Self {
		EditProfile(self.0.insert("password", password))
	}
	/// Edit the user's email address. Does not apply to bot accounts.
	pub fn email(self, email: &str) -> Self {
		EditProfile(self.0.insert("email", email))
	}
	/// Edit the user's password. Does not apply to bot accounts.
	pub fn new_password(self, password: &str) -> Self {
		EditProfile(self.0.insert("new_password", password))
	}
}

fn retry<'a, F: Fn() -> hyper::client::RequestBuilder<'a>>(f: F) -> Result<hyper::client::Response> {
	let f2 = || check_status(f()
		.header(hyper::header::UserAgent(USER_AGENT.to_owned()))
		.send());
	// retry on a ConnectionAborted, which occurs if it's been a while since the last request
	match f2() {
		Err(Error::Hyper(hyper::error::Error::Io(ref io)))
			if io.kind() == std::io::ErrorKind::ConnectionAborted => f2(),
		other => other
	}
}

fn check_status(response: hyper::Result<hyper::client::Response>) -> Result<hyper::client::Response> {
	let response = try!(response);
	if !response.status.is_success() {
		return Err(Error::from_response(response))
	}
	Ok(response)
}

fn resolve_invite(invite: &str) -> &str {
	if invite.starts_with("http://discord.gg/") {
		&invite[18..]
	} else if invite.starts_with("https://discord.gg/") {
		&invite[19..]
	} else if invite.starts_with("discord.gg/") {
		&invite[11..]
	} else {
		invite
	}
}

fn sleep_ms(millis: u64) {
	std::thread::sleep(std::time::Duration::from_millis(millis))
}

// Timer that remembers when it is supposed to go off
struct Timer {
	next_tick_at: time::Timespec,
	tick_len: time::Duration,
}

impl Timer {
	fn new(tick_len_ms: u64) -> Timer {
		let tick_len = time::Duration::milliseconds(tick_len_ms as i64);
		Timer {
			next_tick_at: time::get_time() + tick_len,
			tick_len: tick_len,
		}
	}

	fn immediately(&mut self) {
		self.next_tick_at = time::get_time();
	}

	fn defer(&mut self) {
		self.next_tick_at = time::get_time() + self.tick_len;
	}

	fn check_tick(&mut self) -> bool {
		time::get_time() >= self.next_tick_at && {
			self.next_tick_at = self.next_tick_at + self.tick_len; true
		}
	}

	fn sleep_until_tick(&mut self) {
		let difference = self.next_tick_at - time::get_time();
		if difference > time::Duration::zero() {
			sleep_ms(difference.num_milliseconds() as u64)
		}
		self.next_tick_at = self.next_tick_at + self.tick_len;
	}
}

mod internal {
	pub enum Status {
		SetGame(Option<::model::Game>),
		SendMessage(::serde_json::Value),
	}
}
