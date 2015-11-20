extern crate howl;

use howl::Discord;

fn main() {
	let discord = Discord::new("", "").expect("login failed");
	
	let test_zone = howl::ChannelId("".into());
	//println!("{:#?}", discord.send_message(&test_zone, "Hello from Rust", &[], "", false));
	discord.broadcast_typing(&test_zone).expect("broadcast typing failed");
	
	let mut connection = discord.connect().expect("connect failed");
	loop {
		println!("    {:?}", connection.recv_message());
	}
	
	//discord.logout().expect("logout failed");
}
