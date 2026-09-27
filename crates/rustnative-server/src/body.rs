//! Response bodies: whole, or streamed as the application produces them
//! (a page whose slow parts follow its first bytes, Web milestone H).
//!
//! The pipeline's [`crate::Response`] holds its body as bytes; a handler
//! that streams returns a response made by [`streamed`], whose bytes are
//! empty and whose chunks arrive through the [`Chunks`] sender. The server
//! sends each chunk as it comes.

use std::convert::Infallible;
use std::pin::Pin;
use std::sync::{Arc, Mutex, PoisonError};
use std::task::{Context, Poll};

use bytes::Bytes;
use http_body::Frame;
use tokio::sync::mpsc;

use crate::response::Response;

/// The body the server sends.
#[derive(Debug)]
pub enum Body {
    /// All of it at once.
    Full(Option<Bytes>),
    /// Chunks as they are produced; the body ends when the sender is
    /// dropped.
    Stream(mpsc::Receiver<Bytes>),
}

impl From<Bytes> for Body {
    fn from(bytes: Bytes) -> Self {
        Self::Full(Some(bytes))
    }
}

impl From<&'static str> for Body {
    fn from(text: &'static str) -> Self {
        Self::Full(Some(Bytes::from_static(text.as_bytes())))
    }
}

impl From<String> for Body {
    fn from(text: String) -> Self {
        Self::Full(Some(Bytes::from(text)))
    }
}

impl http_body::Body for Body {
    type Data = Bytes;
    type Error = Infallible;

    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, Infallible>>> {
        match self.get_mut() {
            Self::Full(bytes) => Poll::Ready(
                bytes.take().filter(|bytes| !bytes.is_empty()).map(|bytes| Ok(Frame::data(bytes))),
            ),
            Self::Stream(receiver) => {
                receiver.poll_recv(cx).map(|chunk| chunk.map(|bytes| Ok(Frame::data(bytes))))
            }
        }
    }

    fn is_end_stream(&self) -> bool {
        matches!(self, Self::Full(None))
    }
}

/// The sending half of a streamed response.
#[derive(Debug, Clone)]
pub struct Chunks(mpsc::Sender<Bytes>);

impl Chunks {
    /// Sends `chunk`; `false` when the client has gone (stop producing).
    pub fn send(&self, chunk: impl Into<Bytes>) -> bool {
        self.0.blocking_send(chunk.into()).is_ok()
    }

    /// Sends `chunk` from async code; `false` when the client has gone.
    pub async fn send_async(&self, chunk: impl Into<Bytes>) -> bool {
        self.0.send(chunk.into()).await.is_ok()
    }
}

/// A streamed body waiting in a response's extensions for the server.
#[derive(Clone)]
pub(crate) struct Streaming(Arc<Mutex<Option<mpsc::Receiver<Bytes>>>>);

impl Streaming {
    pub(crate) fn take(&self) -> Option<mpsc::Receiver<Bytes>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner).take()
    }
}

/// A response whose body is streamed: `response`'s status and headers,
/// and the chunks sent through the returned [`Chunks`].
#[must_use]
pub fn streamed(mut response: Response) -> (Response, Chunks) {
    let (sender, receiver) = mpsc::channel(16);
    response.extensions_mut().insert(Streaming(Arc::new(Mutex::new(Some(receiver)))));
    (response, Chunks(sender))
}

/// `response` with the body the server sends.
pub(crate) fn into_body(response: Response) -> http::Response<Body> {
    let stream = response.extensions().get::<Streaming>().and_then(Streaming::take);
    match stream {
        Some(receiver) => response.map(|_| Body::Stream(receiver)),
        None => response.map(Body::from),
    }
}
