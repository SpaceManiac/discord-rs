discord-rs
==========

**discord-rs** is a [Rust](https://rust-lang.org) client library for the
[Discord](https://discordapp.com) chat client's API.

The Discord API can be divided into three main components: the RESTful API
to which calls can be made to take actions, a websocket-based permanent
connection over which state updates are received, and the voice calling
system.

Log in to Discord with `Discord::new`. The resulting value can be used to
make REST API calls to post messages and manipulate Discord state. Calling
`connect()` will open a websocket connection, through which events can be
received. These two channels are enough to write a simple chatbot which can
read and respond to messages.

For more in-depth tracking of Discord state, a `State` can be seeded with
the `ReadyEvent` obtained when opening a `Connection` and kept updated with
the events received over it.

To use the voice call system, initialize a `VoiceConnection` with the user id
received in the `ReadyEvent`, call `voice_connect` on the `Connection`, and
pass events to `VoiceConnection::update`. Once the connection has been
established, audio queued through `push_pcm` and `push_file` will be sent over
the voice connection.

For further details, browse the [source](src/) or use `cargo doc` to read
the documentation. For examples, see the [examples](examples/) directory.
