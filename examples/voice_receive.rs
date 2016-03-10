extern crate discord;

use std::env;
use discord::{Discord, State};
use discord::voice::{VoiceConnection, AudioReceiver};
use discord::model::{Event, UserId};

// A simple voice listener example.
// Use by issuing the "!listen" command in a PM. The bot will join your voice channel and begin
// printing debug information about speaking in the channel. "!listen quit" will cause the bot
// to leave the voice channel.

struct VoiceTest;

impl AudioReceiver for VoiceTest {
	fn speaking_update(&mut self, ssrc: u32, user_id: &UserId, speaking: bool) {
		println!("[{}] is {:?} -> {}", ssrc, user_id, speaking);
	}

	fn voice_packet(&mut self, ssrc: u32, sequence: u16, timestamp: u32, data: &[i16]) {
		println!("[{}] ({}, {}) stereo = {}", ssrc, sequence, timestamp, data.len() == 1920);
	}
}

pub fn main() {
	// log in to the API
	let args: Vec<_> = env::args().collect();
	let discord = Discord::new_cache(
		"tokens.txt",
		args.get(1).expect("No email specified"),
		args.get(2).map(|x| &**x),
	).expect("Login failed");

	// establish websocket and voice connection
	let (mut connection, ready) = discord.connect().expect("connect failed");
	println!("[Ready] {} is serving {} servers", ready.user.username, ready.servers.len());
	let mut voice = VoiceConnection::new(ready.user.id.clone());
	let mut state = State::new(ready);
	let mut current_channel = None;

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
				continue
			},
		};
		state.update(&event);
		voice.update(&event);

		match event {
			Event::Closed(n) => {
				println!("[Error] Connection closed with status: {}", n);
				break
			},
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
					if argument.eq_ignore_ascii_case("quit") || argument.eq_ignore_ascii_case("stop") {
						connection.voice_disconnect();
					} else {
						let output = (|| {
							for server in state.servers() {
								for vstate in &server.voice_states {
									if vstate.user_id == message.author.id {
										if let Some(ref chan) = vstate.channel_id {
											connection.voice_connect(&server.id, chan);
											voice.set_receiver(Box::new(VoiceTest));
											return String::new()
										}
									}
								}
							}
							"You must be in a voice channel to use !listen".into()
						})();
						if output.len() > 0 {
							warn(discord.send_message(&message.channel_id, &output, "", false));
						}
					}
				}
			}
			Event::VoiceStateUpdate(server_id, voice_state) => {
				if voice_state.user_id == state.user().id {
					current_channel = voice_state.channel_id.map(|c| (server_id, c));
				} else if let Some((ref cur_server, ref cur_channel)) = current_channel {
					// If we are in a voice channel, && someone on our server moves/hangs up,
					if *cur_server == server_id {
						// && our current voice channel is empty, disconnect from voice
						if let Some(srv) = state.servers().iter().find(|srv| srv.id == server_id) {
							if srv.voice_states.iter().filter(|vs| vs.channel_id.as_ref() == Some(cur_channel)).count() <= 1 {
								connection.voice_disconnect();
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
