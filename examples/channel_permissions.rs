extern crate discord;

use discord::model::{
	ChannelId,
	PermissionOverwriteType,
	PermissionOverwrite,
	RoleId,
	UserId,
	permissions
};
use discord::Discord;
use std::env;

fn main() {
	// Log in to Discord using a bot token from the environment
	let discord = Discord::from_bot_token(
		&env::var("DISCORD_TOKEN").expect("Expected token"),
	).expect("login failed");
	println!("Ready.");

	// Create bitflags of the permissions to allow and deny
	let allow = permissions::VOICE_CONNECT | permissions::VOICE_SPEAK;
	let deny = permissions::VOICE_MUTE_MEMBERS | permissions::VOICE_MOVE_MEMBERS;

	let channel_id = ChannelId(0); // the id of the channel to set permissions

	// Permissions on a channel can be set for either a member or a role
	//
	// Setting the permissions for a member:
	let user_id = UserId(0); // the id of the user to set permissions for
	let target = PermissionOverwrite {
		kind: PermissionOverwriteType::Member(user_id),
		allow: allow,
		deny: deny,
	};
	println!("{:?}", discord.create_permission(channel_id, target));

	// Similarly, setting the permissions for a role:
	let role_id = RoleId(0); // the id of the role to set permissions for
	let target = PermissionOverwrite {
		kind: PermissionOverwriteType::Role(role_id),
		allow: allow,
		deny: deny,
	};
	println!("{:?}", discord.create_permission(channel_id, target));

	// Deleting all of the permissions for a role or member by passing in the
	// channel id and target member or role:
	let target = PermissionOverwriteType::Member(user_id);
	println!("{:?}", discord.delete_permission(channel_id, target));
}
