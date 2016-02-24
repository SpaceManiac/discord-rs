use super::model::*;

/// The websocket protocol version expected.
const VERSION: u64 = 3;

/// Known state composed from received events.
#[derive(Debug, Clone)]
pub struct State {
	user: CurrentUser,
	settings: UserSettings,
	server_settings: Vec<UserServerSettings>,
	session_id: String,
	private_channels: Vec<PrivateChannel>,
	servers: Vec<LiveServer>,
}

impl State {
	/// Create a new state from an initial `ReadyEvent`.
	pub fn new(ready: ReadyEvent) -> State {
		if ready.version != VERSION {
			warn!("Got protocol version {} instead of {}", ready.version, VERSION);
		}
		State {
			user: ready.user,
			settings: ready.user_settings,
			server_settings: ready.user_server_settings,
			session_id: ready.session_id,
			private_channels: ready.private_channels,
			servers: ready.servers,
		}
	}

	/// Update the state according to the changes described in the given event.
	pub fn update(&mut self, event: &Event) {
		match *event {
			Event::Ready(ref ready) => *self = State::new(ready.clone()),
			Event::GatewayChanged(_, ref ready) => *self = State::new(ready.clone()),
			Event::UserUpdate(ref user) => self.user = user.clone(),
			Event::UserSettingsUpdate {
				ref enable_tts_command, ref inline_attachment_media,
				ref inline_embed_media, ref locale,
				ref message_display_compact, ref muted_channels,
				ref render_embeds, ref show_current_game,
				ref theme, ref convert_emoticons,
			} => {
				opt_modify(&mut self.settings.enable_tts_command, enable_tts_command);
				opt_modify(&mut self.settings.inline_attachment_media, inline_attachment_media);
				opt_modify(&mut self.settings.inline_embed_media, inline_embed_media);
				opt_modify(&mut self.settings.locale, locale);
				opt_modify(&mut self.settings.message_display_compact, message_display_compact);
				opt_modify(&mut self.settings.muted_channels, muted_channels);
				opt_modify(&mut self.settings.render_embeds, render_embeds);
				opt_modify(&mut self.settings.show_current_game, show_current_game);
				opt_modify(&mut self.settings.theme, theme);
				opt_modify(&mut self.settings.convert_emoticons, convert_emoticons);
			}
			Event::UserServerSettingsUpdate(ref settings) => {
				self.server_settings.iter_mut().find(|s| s.server_id == settings.server_id).map(|srv| {
					srv.clone_from(settings);
				});
			}
			Event::VoiceStateUpdate(ref server, ref state) => {
				self.servers.iter_mut().find(|s| s.id == *server).map(|srv| {
					if !state.channel_id.is_some() {
						// Remove the user from the voice state list
						srv.voice_states.retain(|v| v.user_id != state.user_id);
					} else {
						// Update or add to the voice state list
						match srv.voice_states.iter_mut().find(|u| u.user_id == state.user_id) {
							Some(srv_state) => { srv_state.clone_from(state); return }
							None => {}
						}
						srv.voice_states.push(state.clone());
					}
				});
			}
			Event::PresenceUpdate { ref server_id, ref presence, ref user, roles: _ } => {
				self.servers.iter_mut().find(|s| s.id == *server_id).map(|srv| {
					// If the user was modified, update the member list
					if let &Some(ref user) = user {
						srv.members.iter_mut().find(|u| u.user.id == user.id).map(|member| {
							member.user.clone_from(&user);
						});
					}
					if presence.status == OnlineStatus::Offline {
						// Remove the user from the presence list
						srv.presences.retain(|u| u.user_id != presence.user_id);
					} else {
						// Update or add to the presence list
						match srv.presences.iter_mut().find(|u| u.user_id == presence.user_id) {
							Some(srv_presence) => { srv_presence.clone_from(presence); return }
							None => {}
						}
						srv.presences.push(presence.clone());
					}
				});
			}
			Event::ServerCreate(ref server) => self.servers.push(server.clone()),
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
			Event::ServerDelete(ref server) => self.servers.retain(|s| s.id != server.id),
			Event::ServerMemberAdd { ref server_id, ref joined_at, ref roles, ref user } => {
				self.servers.iter_mut().find(|s| s.id == *server_id).map(|srv| {
					srv.member_count += 1;
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
					srv.member_count -= 1;
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

	/// Get information about the logged-in user.
	#[inline]
	pub fn user(&self) -> &CurrentUser { &self.user }

	/// Get the logged-in user's client settings.
	#[inline]
	pub fn settings(&self) -> &UserSettings { &self.settings }

	/// Get the websocket session ID.
	#[inline]
	pub fn session_id(&self) -> &str { &self.session_id }

	/// Get the list of private channels with other users.
	#[inline]
	pub fn private_channels(&self) -> &[PrivateChannel] { &self.private_channels }

	/// Get the list of servers this user has access to.
	#[inline]
	pub fn servers(&self) -> &[LiveServer] { &self.servers }

	/// Look up a private or public channel by its ID.
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

/// A reference to a private or public channel
#[derive(Debug, Clone, Copy)]
pub enum ChannelRef<'a> {
	/// A private channel
	Private(&'a PrivateChannel),
	/// A public channel and its server
	Public(&'a LiveServer, &'a PublicChannel),
}

#[inline]
fn opt_modify<T: Clone>(dest: &mut T, src: &Option<T>) {
	if let Some(val) = src.as_ref() {
		dest.clone_from(val);
	}
}
