extern crate discord;

use std::env;
use discord::{Discord, State};
use discord::voice::AudioReceiver;
use discord::model::{Event, UserId};

// A simple voice listener example.
// Use by issuing the "!listen" command in a PM. The bot will join your voice channel and begin
// printing debug information about speaking in the channel. "!listen quit" will cause the bot
// to leave the voice channel.

struct VoiceTest;

impl AudioReceiver for VoiceTest {
	fn speaking_update(&mut self, ssrc: u32, user_id: UserId, speaking: bool) {
		println!("[{}] is {:?} -> {}", ssrc, user_id, speaking);
	}

	fn voice_packet(&mut self, ssrc: u32, sequence: u16, timestamp: u32, stereo: bool, _data: &[i16]) {
		println!("[{}] ({}, {}) stereo = {}", ssrc, sequence, timestamp, stereo);
	}
}

pub fn main() {
	// log in to the API
	let discord = Discord::from_bot_token(
		&env::var("DISCORD_TOKEN").expect("Expected token"),
	).expect("Login failed");

	// establish websocket and voice connection
	let (mut connection, ready) = discord.connect().expect("connect failed");
	println!("[Ready] {} is serving {} servers", ready.user.username, ready.servers.len());
	let mut state = State::new(ready);

	// receive events forever
	loop {
		let event = match connection.recv_event() {
			Ok(event) => event,
			Err(err) => {
				println!("[Warning] Receive error: {:?}", err);
				if let discord::Error::WebSocket(..) = err {
					// Handle the websocket connection being dropped
					let (new_connection, ready) = discord.connect().expect("connect failed");
					connection = new_connection;
					state = State::new(ready);
					println!("[Ready] Reconnected successfully.");
				}
				if let discord::Error::Closed(..) = err {
					break
				}
				continue
			},
		};
		state.update(&event);

		match event {
			Event::MessageCreate(message) => {
				use std::ascii::AsciiExt;
				// safeguard: stop if the message is from us
				if message.author.id == state.user().id {
					continue
				}

				// reply to a command if there was one
				let mut split = message.content.split(" ");
				let first_word = split.next().unwrap_or("");
				let argument = split.next().unwrap_or("");

				if first_word.eq_ignore_ascii_case("!listen") {
					let voice_channel = state.find_voice_user(message.author.id);
					if argument.eq_ignore_ascii_case("quit") || argument.eq_ignore_ascii_case("stop") {
						if let Some((server_id, _)) = voice_channel {
							connection.drop_voice(server_id);
						}
					} else {
						if let Some((server_id, channel_id)) = voice_channel {
							let voice = connection.voice(server_id);
							voice.connect(channel_id);
							voice.set_receiver(Box::new(VoiceTest));
						} else {
							warn(discord.send_message(message.channel_id, "You must be in a voice channel.", "", false));
						}
					}
				}
			}
			Event::VoiceStateUpdate(server_id, _) => {
				// If someone moves/hangs up, and we are in a voice channel,
				if let Some(cur_channel) = connection.voice(server_id).current_channel() {
					// and our current voice channel is empty, disconnect from voice
					match server_id {
						Some(server_id) => if let Some(srv) = state.servers().iter().find(|srv| srv.id == server_id) {
							if srv.voice_states.iter().filter(|vs| vs.channel_id == Some(cur_channel)).count() <= 1 {
								connection.voice(Some(server_id)).disconnect();
							}
						},
						None => if let Some(call) = state.calls().get(&cur_channel) {
							if call.voice_states.len() <= 1 {
								connection.voice(server_id).disconnect();
							}
						}
					}
				}
			}
			_ => {}, // discard other events
		}
	}
}

#[allow(dead_code)]
fn warn<T, E: ::std::fmt::Debug>(result: Result<T, E>) {
	match result {
		Ok(_) => {},
		Err(err) => println!("[Warning] {:?}", err)
	}
}
