extern crate discord;

use discord::Discord;

fn main() {
	let discord = Discord::new("", "").expect("login failed");
	
	let test_zone = discord::ChannelId("".into());
	//println!("{:#?}", discord.send_message(&test_zone, "Hello from Rust", &[], "", false));
	discord.broadcast_typing(&test_zone).expect("broadcast typing failed");
	
	let conn = discord.connect().expect("connect failed");
	
	discord.logout().expect("logout failed");
}
