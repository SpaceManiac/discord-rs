use std::collections::BTreeMap;
use super::model::*;

/// Known state composed from received events.
#[derive(Debug, Clone)]
pub struct State {
	user: CurrentUser,
	servers: Vec<LiveServer>,
	unavailable_servers: Vec<ServerId>,
	private_channels: Vec<PrivateChannel>,
	groups: BTreeMap<ChannelId, Group>,
	calls: BTreeMap<ChannelId, Call>,
	presences: Vec<Presence>,
	relationships: Vec<Relationship>,
	// user accounts only
	settings: Option<UserSettings>,
	server_settings: Option<Vec<UserServerSettings>>,
	notes: Option<BTreeMap<UserId, String>>,
}

impl State {
	/// Create a new state from an initial `ReadyEvent`.
	pub fn new(ready: ReadyEvent) -> State {
		let mut servers = Vec::new();
		let mut unavailable = Vec::new();
		for server in ready.servers {
			match server {
				PossibleServer::Offline(id) => unavailable.push(id),
				PossibleServer::Online(id) => servers.push(id),
			}
		}
		let mut groups: BTreeMap<ChannelId, Group> = BTreeMap::new();
		let mut private_channels: Vec<PrivateChannel> = vec![];
		for channel in ready.private_channels {
			match channel {
				Channel::Private(channel) => private_channels.push(channel),
				Channel::Group(group) => { groups.insert(group.channel_id, group); },
				_ => {},
			}
		}
		State {
			user: ready.user,
			servers: servers,
			unavailable_servers: unavailable,
			private_channels: private_channels,
			groups: groups,
			calls: BTreeMap::new(),
			presences: ready.presences,
			relationships: ready.relationships,
			settings: ready.user_settings,
			server_settings: ready.user_server_settings,
			notes: ready.notes,
		}
	}

	/// Count the total number of server members not yet downloaded.
	pub fn unknown_members(&self) -> u64 {
		let mut total = 0;
		for srv in &self.servers {
			let members = srv.members.len() as u64;
			if srv.member_count > members {
				total += srv.member_count - members;
			} else if srv.member_count < members {
				warn!("Inconsistent member count for {:?}: {} < {}", srv.id, srv.member_count, members);
			}
		}
		total
	}

	/// Build a list of all known private and group channel IDs.
	pub fn all_private_channels(&self) -> Vec<ChannelId> {
		self.groups.keys().cloned().chain(self.private_channels.iter().map(|c| c.id)).collect()
	}

	/// Build a list of all known server IDs.
	pub fn all_servers(&self) -> Vec<ServerId> {
		self.servers.iter().map(|s| s.id).chain(self.unavailable_servers.iter().cloned()).collect()
	}

	#[doc(hidden)]
	pub fn __download_members(&mut self) -> Vec<ServerId> {
		self.servers.iter_mut()
			.filter(|s| s.large)
			.map(|s| { s.members.clear(); s.id })
			.collect::<Vec<_>>()
	}

	/// Update the state according to the changes described in the given event.
	pub fn update(&mut self, event: &Event) {
		match *event {
			Event::Ready(ref ready) => *self = State::new(ready.clone()),
			Event::UserUpdate(ref user) => self.user = user.clone(),
			Event::UserNoteUpdate(user_id, ref note) => {
				if let Some(notes) = self.notes.as_mut() {
					if note.is_empty() {
						notes.remove(&user_id);
					} else {
						notes.insert(user_id, note.clone());
					}
				}
			},
			Event::UserSettingsUpdate {
				ref detect_platform_accounts, ref developer_mode,
				ref enable_tts_command, ref inline_attachment_media,
				ref inline_embed_media, ref locale,
				ref message_display_compact,
				ref render_embeds, ref server_positions,
				ref show_current_game, ref status,
				ref theme, ref convert_emoticons,
				ref friend_source_flags,
			} => {
				if let Some(settings) = self.settings.as_mut() {
					opt_modify(&mut settings.detect_platform_accounts, detect_platform_accounts);
					opt_modify(&mut settings.developer_mode, developer_mode);
					opt_modify(&mut settings.enable_tts_command, enable_tts_command);
					opt_modify(&mut settings.inline_attachment_media, inline_attachment_media);
					opt_modify(&mut settings.inline_embed_media, inline_embed_media);
					opt_modify(&mut settings.locale, locale);
					opt_modify(&mut settings.message_display_compact, message_display_compact);
					opt_modify(&mut settings.render_embeds, render_embeds);
					opt_modify(&mut settings.server_positions, server_positions);
					opt_modify(&mut settings.show_current_game, show_current_game);
					opt_modify(&mut settings.status, status);
					opt_modify(&mut settings.theme, theme);
					opt_modify(&mut settings.convert_emoticons, convert_emoticons);
					opt_modify(&mut settings.friend_source_flags, friend_source_flags);
				}
			}
			Event::UserServerSettingsUpdate(ref settings) => {
				if let Some(server_settings) = self.server_settings.as_mut() {
					server_settings.iter_mut().find(|s| s.server_id == settings.server_id).map(|srv| {
						srv.clone_from(settings);
					});
				}
			}
			Event::VoiceStateUpdate(None, ref state) => {
				if let Some(channel) = state.channel_id {
					// channel id available, insert voice state
					if let Some(call) = self.calls.get_mut(&channel) {
						if let Some(grp_state) = call.voice_states.iter_mut().find(|u| u.user_id == state.user_id) {
							grp_state.clone_from(state);
							return
						}
						call.voice_states.push(state.clone());
					}
				} else {
					// delete this user from any group call containing them
					for call in self.calls.values_mut() {
						call.voice_states.retain(|u| u.user_id != state.user_id);
					}
				}
			}
			Event::VoiceStateUpdate(Some(server_id), ref state) => {
				self.servers.iter_mut().find(|s| s.id == server_id).map(|srv| {
					if !state.channel_id.is_some() {
						// Remove the user from the voice state list
						srv.voice_states.retain(|v| v.user_id != state.user_id);
					} else {
						// Update or add to the voice state list
						if let Some(srv_state) = srv.voice_states.iter_mut().find(|u| u.user_id == state.user_id) {
							srv_state.clone_from(state);
							return
						}
						srv.voice_states.push(state.clone());
					}
				});
			}
			Event::CallCreate(ref call) => {
				use std::collections::btree_map::Entry;
				match self.calls.entry(call.channel_id) {
					Entry::Vacant(e) => { e.insert(call.clone()); }
					Entry::Occupied(mut e) => { e.get_mut().clone_from(call); }
				}
			}
			Event::CallUpdate { channel_id, message_id: _, ref region, ref ringing } => {
				if let Some(call) = self.calls.get_mut(&channel_id) {
					call.region.clone_from(region);
					call.ringing.clone_from(ringing);
				}
			}
			Event::CallDelete(channel_id) => {
				self.calls.remove(&channel_id);
			}
			Event::ChannelRecipientAdd(channel_id, ref user) => {
				if let Some(group) = self.groups.get_mut(&channel_id) {
					group.recipients.push(user.clone());
				}
			}
			Event::ChannelRecipientRemove(channel_id, ref user) => {
				if let Some(group) = self.groups.get_mut(&channel_id) {
					group.recipients.retain(|u| u.id != user.id);
				}
			}
			Event::PresenceUpdate { server_id, ref presence, roles: _ } => {
				if let Some(server_id) = server_id {
					self.servers.iter_mut().find(|s| s.id == server_id).map(|srv| {
						// If the user was modified, update the member list
						if let Some(user) = presence.user.as_ref() {
							srv.members.iter_mut().find(|u| u.user.id == user.id).map(|member| {
								member.user.clone_from(user);
							});
						}
						update_presence(&mut srv.presences, presence);
					});
				} else {
					update_presence(&mut self.presences, presence);
				}
			}
			Event::PresencesReplace(ref presences) => {
				self.presences.clone_from(presences);
			}
			Event::RelationshipAdd(ref relationship) => {
				if let Some(rel) = self.relationships.iter_mut().find(|r| r.id == relationship.id) {
					rel.clone_from(relationship); return
				}
				self.relationships.push(relationship.clone());
			}
			Event::RelationshipRemove(user_id, _) => {
				self.relationships.retain(|r| r.id != user_id);
			}
			Event::ServerCreate(PossibleServer::Offline(server_id)) |
			Event::ServerDelete(PossibleServer::Offline(server_id)) => {
				self.servers.retain(|s| s.id != server_id);
				if !self.unavailable_servers.contains(&server_id) {
					self.unavailable_servers.push(server_id);
				}
			}
			Event::ServerCreate(PossibleServer::Online(ref server)) => {
				self.unavailable_servers.retain(|&id| id != server.id);
				self.servers.push(server.clone())
			}
			Event::ServerDelete(PossibleServer::Online(ref server)) => {
				self.servers.retain(|s| s.id != server.id);
			}
			Event::ServerUpdate(ref server) => {
				self.servers.iter_mut().find(|s| s.id == server.id).map(|srv| {
					srv.name.clone_from(&server.name);
					srv.afk_timeout = server.afk_timeout;
					srv.afk_channel_id.clone_from(&server.afk_channel_id);
					srv.icon.clone_from(&server.icon);
					srv.roles.clone_from(&server.roles);
					srv.region.clone_from(&server.region);
					// embed_enabled and embed_channel_id skipped
					srv.owner_id.clone_from(&server.owner_id);
					srv.verification_level = server.verification_level;
				});
			}
			Event::ServerMemberAdd(ref server_id, ref member) => {
				self.servers.iter_mut().find(|s| s.id == *server_id).map(|srv| {
					srv.member_count += 1;
					srv.members.push(member.clone());
				});
			}
			Event::ServerMemberUpdate { ref server_id, ref roles, ref user, ref nick } => {
				self.servers.iter_mut().find(|s| s.id == *server_id).map(|srv| {
					srv.members.iter_mut().find(|m| m.user.id == user.id).map(|member| {
						member.user.clone_from(user);
						member.roles.clone_from(roles);
						member.nick.clone_from(nick);
					})
				});
			}
			Event::ServerMemberRemove(ref server_id, ref user) => {
				self.servers.iter_mut().find(|s| s.id == *server_id).map(|srv| {
					srv.member_count -= 1;
					srv.members.retain(|m| m.user.id != user.id);
				});
			}
			Event::ServerMembersChunk(server_id, ref members) => {
				self.servers.iter_mut().find(|s| s.id == server_id).map(|srv| {
					srv.members.extend_from_slice(members);
				});
			}
			Event::ServerSync { server_id, large, ref members, ref presences } => {
				self.servers.iter_mut().find(|s| s.id == server_id).map(|srv| {
					srv.large = large;
					srv.members.clone_from(members);
					srv.presences.clone_from(presences);
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
				Channel::Group(ref group) => {
					self.groups.insert(group.channel_id, group.clone());
				}
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
				Channel::Group(ref group) => {
					use std::collections::btree_map::Entry;
					match self.groups.entry(group.channel_id) {
						Entry::Vacant(e) => { e.insert(group.clone()); }
						Entry::Occupied(mut e) => {
							let dest = e.get_mut();
							if group.recipients.is_empty() {
								// if the update omits the recipient list, preserve it
								let recipients = ::std::mem::replace(&mut dest.recipients, Vec::new());
								dest.clone_from(group);
								dest.recipients = recipients;
							} else {
								dest.clone_from(group);
							}
						}
					}
				}
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
				Channel::Group(ref group) => {
					self.groups.remove(&group.channel_id);
				}
				Channel::Private(ref channel) => {
					self.private_channels.retain(|c| c.id != channel.id);
				}
				Channel::Public(ref channel) => {
					self.servers.iter_mut().find(|s| s.id == channel.server_id).map(|srv| {
						srv.channels.retain(|c| c.id != channel.id);
					});
				}
			},
			Event::ChannelPinsUpdate { ref channel_id, ref last_pin_timestamp } => {
				for server in &mut self.servers {
					for channel in &mut server.channels {
						if channel.id == *channel_id {
							channel.last_pin_timestamp = last_pin_timestamp.clone();
							return
						}
					}
				}

				for channel in &mut self.private_channels {
					if channel.id == *channel_id {
						channel.last_pin_timestamp = last_pin_timestamp.clone();
						return
					}
				}

				if let Some(group) = self.groups.get_mut(channel_id) {
					group.last_pin_timestamp = last_pin_timestamp.clone();
					return
				}
			}
			_ => {}
		}
	}

	/// Get information about the logged-in user.
	#[inline]
	pub fn user(&self) -> &CurrentUser { &self.user }

	/// Get the servers this user has access to.
	#[inline]
	pub fn servers(&self) -> &[LiveServer] { &self.servers }

	/// Get the currently unavailable servers.
	pub fn unavailable_servers(&self) -> &[ServerId] { &self.unavailable_servers }

	/// Get the active 1-on-1 private channels with other users.
	#[inline]
	pub fn private_channels(&self) -> &[PrivateChannel] { &self.private_channels }

	/// Get the active groups with other users.
	#[inline]
	pub fn groups(&self) -> &BTreeMap<ChannelId, Group> { &self.groups }

	/// Get the currently ongoing calls.
	///
	/// Calls which were already ongoing when the user connected may require
	/// calling `Connection::sync_calls` to appear here.
	#[inline]
	pub fn calls(&self) -> &BTreeMap<ChannelId, Call> { &self.calls }

	/// Get the presences of online friends.
	///
	/// Usually empty for bot accounts.
	#[inline]
	pub fn presences(&self) -> &[Presence] { &self.presences }

	/// Get the friend/block relationships with other users.
	///
	/// Usually empty for bot accounts.
	#[inline]
	pub fn relationships(&self) -> &[Relationship] { &self.relationships }

	/// Get the logged-in user's client settings.
	///
	/// Returns `None` on bot accounts.
	#[inline]
	pub fn settings(&self) -> Option<&UserSettings> { self.settings.as_ref() }

	/// Get the logged-in user's per-server notification settings.
	///
	/// Returns `None` on bot accounts.
	#[inline]
	pub fn server_settings(&self) -> Option<&[UserServerSettings]> { self.server_settings.as_ref().map(|x| &x[..]) }

	/// Get the map of notes that have been made by this user.
	///
	/// Returns `None` on bot accounts.
	#[inline]
	pub fn notes(&self) -> Option<&BTreeMap<UserId, String>> { self.notes.as_ref() }

	/// Look up a private or public channel by its ID.
	pub fn find_channel(&self, id: ChannelId) -> Option<ChannelRef> {
		for server in &self.servers {
			for channel in &server.channels {
				if channel.id == id {
					return Some(ChannelRef::Public(server, channel))
				}
			}
		}
		for channel in &self.private_channels {
			if channel.id == id {
				return Some(ChannelRef::Private(channel))
			}
		}
		if let Some(group) = self.groups.get(&id) {
			return Some(ChannelRef::Group(group))
		}
		None
	}

	/// Look up the voice channel or group call a user is in, if any.
	///
	/// For bot users which may be in multiple voice channels, the first found is returned.
	pub fn find_voice_user(&self, user_id: UserId) -> Option<(Option<ServerId>, ChannelId)> {
		for server in &self.servers {
			for vstate in &server.voice_states {
				if vstate.user_id == user_id {
					if let Some(channel_id) = vstate.channel_id {
						return Some((Some(server.id), channel_id));
					}
				}
			}
		}
		for call in self.calls.values() {
			for vstate in &call.voice_states {
				if vstate.user_id == user_id {
					return Some((None, call.channel_id));
				}
			}
		}
		None
	}
}

fn update_presence(vec: &mut Vec<Presence>, presence: &Presence) {
	if presence.status == OnlineStatus::Offline {
		// Remove the user from the presence list
		vec.retain(|u| u.user_id != presence.user_id);
	} else {
		// Update or add to the presence list
		if let Some(srv_presence) = vec.iter_mut().find(|u| u.user_id == presence.user_id) {
			if presence.user.is_none() {
				let user = srv_presence.user.clone();
				srv_presence.clone_from(presence);
				srv_presence.user = user;
			} else {
				srv_presence.clone_from(presence);
			}
			return
		}
		vec.push(presence.clone());
	}
}

/// A reference to a private or public channel.
#[derive(Debug, Clone, Copy)]
pub enum ChannelRef<'a> {
	/// A private channel
	Private(&'a PrivateChannel),
	/// A group channel
	Group(&'a Group),
	/// A public channel and its server
	Public(&'a LiveServer, &'a PublicChannel),
}

#[inline]
fn opt_modify<T: Clone>(dest: &mut T, src: &Option<T>) {
	if let Some(val) = src.as_ref() {
		dest.clone_from(val);
	}
}
