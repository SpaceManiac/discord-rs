use super::imports::*;
use websocket::{OwnedMessage, WebSocketError};

/// This wrapper takes a websocket client, and automatically handles the Ping events sent by the
/// websocket server.
pub struct PingFilter<Client> {
    upstream: Client,
    // If we receive a ping but our outbound buffer is full, we'll need to stash the pending ping
    // here. We stash only one such ping; if we receive multiple pings while the send side is
    // blocked we'll send only the latest.
    pending_pong: Option<OwnedMessage>
}

impl<Client: WSClientish> PingFilter<Client> {
    pub fn new(client: Client) -> PingFilter<Client> {
        PingFilter { upstream: client, pending_pong: None }
    }
}

impl<T: WSClientish> Stream for PingFilter<T>
    where Self: Sink<SinkItem=OwnedMessage, SinkError=WebSocketError>
{
    type Item=T::Item;
    type Error=T::Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        // Poll the sink side to advance any potential pending pings.
        // We don't actually care about the result unless it's an error.
        self.poll_complete()?;

        match self.upstream.poll() {
            Ok(Async::Ready(Some(OwnedMessage::Ping(data)))) => {
                self.pending_pong = Some(OwnedMessage::Pong(data));
                // Look for another message immediately; this will also kick off sending the pong.
                self.poll()
            }
            something => something
        }
    }
}

impl<T: WSClientish> Sink for PingFilter<T> {
    type SinkItem = T::SinkItem;
    type SinkError = T::SinkError;

    fn start_send(&mut self, item: Self::SinkItem) -> StartSend<Self::SinkItem, Self::SinkError> {
        self.upstream.start_send(item)
    }

    fn poll_complete(&mut self) -> Poll<(), Self::SinkError> {
        let pong = ::std::mem::replace(&mut self.pending_pong, None);

        if pong.is_some() {
            match self.upstream.start_send(pong.unwrap())? {
                AsyncSink::NotReady(pong) => { self.pending_pong = Some(pong); }
                _ => {}
            }
        }

        self.upstream.poll_complete()
    }

    fn close(&mut self) -> Poll<(), Self::SinkError> {
        self.upstream.close()
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use super::super::mockstream::*;

    #[test]
    fn ping_pong() {
        in_task(|| {
            let (mut streamctl, stream) = MockStream::new(100);
            let (sink, mut rx, mut sink_state) = MockSink::new(100);
            let combined = Combined::new(stream, sink);

            let mut ping_filter = PingFilter::new(combined);

            // Initial state: Nothing happens
            assert_eq!(ping_filter.poll().unwrap(), Async::NotReady);
            assert_eq!(ping_filter.poll_complete().unwrap(), Async::NotReady);

            // If we send it a message, we should be able to poll it off
            let msg = OwnedMessage::Text("foo".into());
            streamctl.send_item(msg.clone());
            assert_eq!(ping_filter.poll().unwrap(), Async::Ready(Some(msg)));

            // If we send a ping, it should not pop off when we poll, and should be ponged
            // automatically
            let msg = OwnedMessage::Ping(vec![1,2,3]);
            streamctl.send_item(msg.clone());
            assert_eq!(ping_filter.poll().unwrap(), Async::NotReady);
            assert_eq!(rx.poll(), Ok(Async::Ready(Some(OwnedMessage::Pong(vec![1,2,3])))));

            // If the pong can't be sent immediately, we'll still send it later
            *sink_state.lock().unwrap() = MockSinkState::Full;
            streamctl.send_item(msg);
            assert_eq!(rx.poll(), Ok(Async::NotReady));
            ping_filter.poll();
            assert_eq!(rx.poll(), Ok(Async::NotReady));
            *sink_state.lock().unwrap() = MockSinkState::NotFlushed;
            ping_filter.poll();
            assert_eq!(rx.poll(), Ok(Async::Ready(Some(OwnedMessage::Pong(vec![1,2,3])))));
        });
    }
}
