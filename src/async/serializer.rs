use super::imports::*;

use model::GatewayEvent;
use serde_json::{self, Value};
use error::Error;
use futures::{self, stream, sink};
use websocket::OwnedMessage;


/// This filter wraps the stream+sink combo provided by the websockets library to decode incoming
/// GatewayEvents, and encode outgoing serde_json::Values. It also converts errors to our own error
/// type.
pub struct Serializer<T> {
    upstream: T
}

impl<T> Serializer<T> {
    pub fn new(client: T) -> Self {
        Serializer { upstream: client }
    }

    pub fn into_inner(self) -> T {
        self.upstream
    }
}

fn handle_message(msg: OwnedMessage) -> Result<GatewayEvent, Error> {
    match msg {
        OwnedMessage::Ping(_) => Err(Error::Other("Unexpected ping")),
        OwnedMessage::Pong(_) => Err(Error::Other("Unexpected pong")),
        OwnedMessage::Close(None) => Err(Error::Closed(None, "Unexpected close".into())),
        OwnedMessage::Close(Some(info)) => Err(Error::Closed(Some(info.status_code), info.reason)),
        OwnedMessage::Text(text) => handle_data(text.as_bytes()),
        OwnedMessage::Binary(data) => {
            use std::io::Read;
            use flate2;
            let mut payload_vec = Vec::new();
            flate2::read::ZlibDecoder::new(&data[..]).read_to_end(&mut payload_vec)?;

            handle_data(&payload_vec[..])
        }
    }
}

fn handle_data(data: &[u8]) -> Result<GatewayEvent, Error> {
    let value = serde_json::from_slice(data).map_err(Error::from)?;

    GatewayEvent::decode(value)
}

impl<T: WSClientish> Stream for Serializer<T> {
    type Item = GatewayEvent;
    type Error = Error;

    fn poll(&mut self) -> Poll<Option<GatewayEvent>, Error> {
        match self.upstream.poll() {
            Err(e) => Err(From::from(e)),
            Ok(Async::NotReady) => Ok(Async::NotReady),
            Ok(Async::Ready(None)) => Ok(Async::Ready(None)),
            Ok(Async::Ready(Some(message)))
                => handle_message(message).map(|m| Async::Ready(Some(m)))
        }
    }
}

impl<T: WSClientish> Sink for Serializer<T> {
    type SinkItem = serde_json::Value;
    type SinkError = Error;

    fn start_send(&mut self, item: Self::SinkItem) -> StartSend<Self::SinkItem, Self::SinkError> {
        let data = match serde_json::to_vec(&item).map_err(From::from) {
            Err(e) => return Err(e),
            Ok(data) => data
        };

        let str = String::from_utf8(data).unwrap();
        println!("send: {}", str);

        match self.upstream.start_send(OwnedMessage::Text(str)) {
            Err(e) => Err(From::from(e)),
            Ok(AsyncSink::Ready) => { println!(" => ready"); Ok(AsyncSink::Ready) },
            Ok(AsyncSink::NotReady(_)) => Ok(AsyncSink::NotReady(item))
        }
    }

    fn poll_complete(&mut self) -> Poll<(), Self::SinkError> {
        self.upstream.poll_complete().map_err(From::from)
    }

    fn close(&mut self) -> Poll<(), Self::SinkError> {
        self.upstream.close().map_err(From::from)
    }
}

#[cfg(test)]
mod test {
    use futures::{stream, sink};
    use super::*;
    use super::super::imports::*;
    use websocket::WebSocketError;

    fn compress(data: Vec<u8>) -> Vec<u8> {
        use flate2;
        use std::io::Write;
        let mut out = Vec::new();

        flate2::write::ZlibEncoder::new(&mut out, flate2::Compression::Best)
            .write_all(&data[..]).unwrap();

        out
    }

    #[test]
    fn test() {
        in_task(|| {
            let (mut streamctl, stream) = MockStream::new(100);
            let (sink, mut rx, mut sink_state) = MockSink::new(100);
            let combined = Combined::new(stream, sink);

            let mut serializer = Serializer::new(combined);

            // Empty behavior
            assert!(serializer.poll().unwrap().is_not_ready());
            assert!(serializer.poll_complete().unwrap().is_ready());

            // Basic decode
            streamctl.send_item(
                OwnedMessage::Binary(
                    compress(serde_json::to_vec(
                        &json!({"op": 7})
                    ).unwrap())
                ));
            match serializer.poll().unwrap() {
                Async::Ready(Some(GatewayEvent::Reconnect)) => { /* ok */ },
                other => panic!("unexpected: {:?}", other)
            }

            // decode of Text messages
            streamctl.send_item(OwnedMessage::Text("{\"op\": 7}".into()));
            match serializer.poll().unwrap() {
                Async::Ready(Some(GatewayEvent::Reconnect)) => { /* ok */ },
                other => panic!("unexpected: {:?}", other)
            }

            // Basic encode
            let expected = vec![1,2,3];
            let json = Value::from(expected.clone());

            assert!(serializer.start_send(json.clone()).unwrap().is_ready());
            match rx.poll() {
                Ok(Async::Ready(Some(OwnedMessage::Binary(data)))) => {
                    assert_eq!(expected, serde_json::from_slice::<Vec<usize>>(&data[..]).unwrap());
                },
                other => panic!("unexpected {:?}", other)
            }

            // Failed decode
            streamctl.send_item(OwnedMessage::Text("this is not json".into()));
            assert!(serializer.poll().is_err());

            // Blocking send
            (*sink_state.lock().unwrap()) = MockSinkState::Full;
            assert!(serializer.start_send(json.clone()).unwrap().is_not_ready());

            // Failed send
            (*sink_state.lock().unwrap()) = MockSinkState::Err(WebSocketError::ProtocolError("foo"));
            assert!(serializer.start_send(json.clone()).is_err());
        });
    }
}