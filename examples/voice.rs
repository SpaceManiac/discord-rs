extern crate discord;

use discord::Discord;
use discord::voice::{self, VoiceConnection};
use discord::model::{Event, ChannelId, ServerId};
use std::env;

fn main() {
	// Log in to Discord using the email and password in the environment
	let discord = Discord::new(
		&env::var("DISCORD_EMAIL").expect("DISCORD_EMAIL"),
		&env::var("DISCORD_PASSWORD").expect("DISCORD_PASSWORD")
	).expect("login failed");

	// Establish and use a websocket connection
	let (mut connection, ready) = discord.connect().expect("connect failed");
	let mut voice = VoiceConnection::new(ready.user.id.clone());

	connection.voice_connect(
		&ServerId(env::var("DISCORD_SERVER").expect("DISCORD_SERVER")),
		&ChannelId(env::var("DISCORD_CHANNEL").expect("DISCORD_CHANNEL")),
	);

	voice.play(
		voice::open_ffmpeg_stream(
			&env::var("DISCORD_AUDIO").expect("DISCORD_AUDIO")
		).expect("File read failed")
	);

	println!("Ready.");
	loop {
		let event = match connection.recv_event() {
			Ok(event) => event,
			Err(err) => {
				println!("Receive error: {:?}", err);
				break
			}
		};
		voice.update(&event);
		match event {
			Event::Closed(n) => {
				println!("Discord closed on us with status {}", n);
				break
			}
			_ => {}
		}
	}

	// Log out from the API
	discord.logout().expect("logout failed");
}
