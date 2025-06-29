use core::fmt;
use core::future::ready;
use core::net::SocketAddr;
use core::pin::Pin;
use core::task::{Context, Poll, ready};

use futures_util::{SinkExt, Stream, StreamExt};
use log::{error, trace, warn};
use reqwest::Client;
use reqwest_websocket::{CloseCode, Message, RequestBuilderExt, WebSocket};

use crate::Bytes;

/// Wrapper for the WebSocket connection to a Homey Energy Dongle.
///
/// This struct implements [Stream] over [Bytes] buffers received from the dongle. To create a new connection, call
/// [WebsocketEnergyDongle::connect()] with the dongle host details.
pub struct WebsocketEnergyDongle {
	websocket: WebSocket,
}

impl WebsocketEnergyDongle {
	/// Create a new WebSocket connection to a Homey Energy Dongle.
	///
	/// Arguments to this call can be discovered using the [crate::discover::discover_devices_with_mdns()] call.
	///
	/// The Homey Energy Dongle supports a maximum of 2 concurrent connections. When a 3rd connection is attempted, this function will
	/// return `Err(Error::DongleError(DongleError::ConnectionLimitReached))`.
	///
	/// See the [crate-level documentation](crate) for more details and examples.
	pub async fn connect(addr: SocketAddr, path: &str) -> Result<Self, ConnectError> {
		let path = path.strip_prefix('/').unwrap_or(path);
		let url = format!("ws://{addr}/{path}");
		trace!("Connecting to Homey Energy Dongle at {url}...");
		let res = Client::new().get(url).upgrade().send().await?;
		res.error_for_status_ref()?;
		let mut websocket = res.into_websocket().await?;
		websocket.send(Message::Ping(Bytes::new())).await?;
		let mut next_pong_or_close = (&mut websocket).filter(|msg| {
			ready(match msg {
				Err(_) | Ok(Message::Pong(_) | Message::Close { .. }) => true,
				Ok(Message::Text(_) | Message::Binary(_) | Message::Ping(_)) => false,
			})
		});
		let Some(pong) = next_pong_or_close.next().await else {
			return Err(ConnectError::DongleIsNotResponding);
		};
		match pong? {
			Message::Pong(_) => { /* aok */ }
			Message::Text(_) | Message::Binary(_) | Message::Ping(_) => {
				error!("Unexpected message received, should've been filtered")
			}
			Message::Close { code, reason } => {
				return Err(ConnectError::DongleError(DongleError::from_code_and_reason(code, reason)));
			}
		}

		Ok(Self { websocket })
	}
}

impl Stream for WebsocketEnergyDongle {
	type Item = Result<Bytes, StreamError>;

	fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
		let Some(msg_res) = ready!(Pin::new(&mut self.websocket).poll_next(cx)) else {
			return Poll::Ready(None);
		};
		let msg = match msg_res {
			Ok(msg) => msg,
			Err(err) => return Poll::Ready(Some(Err(StreamError::WebSocket(err)))),
		};
		match msg {
			Message::Text(txt) => Poll::Ready(Some(Ok(Bytes::from(txt)))),
			Message::Binary(bin) => Poll::Ready(Some(Ok(bin))),
			Message::Ping(payload) => {
				warn!("Ignoring spurious ping with payload: {payload:?}");
				cx.waker().wake_by_ref();
				Poll::Pending
			}
			Message::Pong(payload) => {
				warn!("Ignoring spurious pong with payload: {payload:?}");
				cx.waker().wake_by_ref();
				Poll::Pending
			}
			Message::Close { code, reason } => Poll::Ready(Some(Err(StreamError::DongleError(DongleError::from_code_and_reason(
				code, reason,
			))))),
		}
	}
}

/// Possible error scenarios for [WebsocketEnergyDongle::connect()].
#[derive(Debug)]
pub enum ConnectError {
	/// Dongle is not responding to the messages
	DongleIsNotResponding,
	/// Dongle-specific error
	DongleError(DongleError),
	/// WebSocket client error
	WebSocket(reqwest_websocket::Error),
	/// HTTP client error
	Http(reqwest::Error),
}

impl From<reqwest_websocket::Error> for ConnectError {
	fn from(err: reqwest_websocket::Error) -> Self {
		Self::WebSocket(err)
	}
}

impl From<reqwest::Error> for ConnectError {
	fn from(err: reqwest::Error) -> Self {
		Self::Http(err)
	}
}

impl fmt::Display for ConnectError {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		match self {
			Self::DongleIsNotResponding => write!(f, "Homey Energy Dongle is not responding"),
			Self::DongleError(err) => write!(f, "Homey Energy Dongle error: {err}, details: {err:?}"),
			Self::WebSocket(err) => write!(f, "WebSocket error: {err}, details: {err:?}"),
			Self::Http(err) => write!(f, "HTTP error: {err}, details: {err:?}"),
		}
	}
}

impl std::error::Error for ConnectError {}

/// Possible error scenarios for [Stream] implementation of [WebsocketEnergyDongle].
#[derive(Debug)]
pub enum StreamError {
	/// Dongle-specific error
	DongleError(DongleError),
	/// WebSocket client error
	WebSocket(reqwest_websocket::Error),
}

impl fmt::Display for StreamError {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		match self {
			Self::DongleError(err) => write!(f, "Homey Energy Dongle error: {err}, details: {err:?}"),
			Self::WebSocket(err) => write!(f, "WebSocket error: {err}, details: {err:?}"),
		}
	}
}

impl std::error::Error for StreamError {}

/// Specific errors returned by the Homey Energy Dongle API.
#[derive(Debug)]
pub enum DongleError {
	/// Connection limit reached
	ConnectionLimitReached,
	/// Local API disabled
	LocalApiDisabled,
	/// Other errors
	Other(String),
}

impl DongleError {
	pub fn from_code_and_reason(code: CloseCode, reason: String) -> Self {
		match code {
			CloseCode::Policy => match reason.as_str() {
				"Connection limit reached" => Self::ConnectionLimitReached,
				"Local API disabled" => Self::LocalApiDisabled,
				_ => Self::Other(reason),
			},
			_ => Self::Other(reason),
		}
	}
}

impl fmt::Display for DongleError {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		match self {
			Self::ConnectionLimitReached => write!(f, "Connection limit reached"),
			Self::LocalApiDisabled => write!(f, "Local API disabled"),
			Self::Other(err) => f.write_str(err),
		}
	}
}
