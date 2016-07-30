discord-rs [![](https://meritbadge.herokuapp.com/discord)](https://crates.io/crates/discord) [![](https://img.shields.io/badge/chat-Discord-blue.svg)](https://discord.gg/0SBTUU1wZTWAPisK) [![](https://img.shields.io/badge/docs-online-2020ff.svg)](http://wombat.platymuus.com/rustdoc/discord_master/)
==========

**discord-rs** is a [Rust](https://www.rust-lang.org) client library for the
[Discord](https://discordapp.com) chat client's API.

The Discord API can be divided into three main components: the RESTful API
to which calls can be made to take actions, a websocket-based permanent
connection over which state updates are received, and the voice calling
system.

Log in to Discord with `Discord::new`, `new_cache`, or `from_bot_token` as
appropriate. The resulting value can be used to make REST API calls to post
messages and manipulate Discord state. Calling `connect()` will open a
websocket connection, through which events can be received. These two channels
are enough to write a simple chatbot which can read and respond to messages.

For more in-depth tracking of Discord state, a `State` can be seeded with
the `ReadyEvent` obtained when opening a `Connection` and kept updated with
the events received over it.

To join voice servers, call `Connection::voice` to get a `VoiceConnection` and use `connect`
to join a channel, then `play` and `stop` to control playback. Manipulating deaf/mute state
and receiving audio are also possible.

For further details, browse the [source](src/) or read
[the documentation](http://wombat.platymuus.com/rustdoc/discord_master/).
Refer to the [installation guide](https://github.com/SpaceManiac/discord-rs/wiki/Windows-Installation)
if you are using Windows. For examples, browse the [examples](examples/)
directory.
