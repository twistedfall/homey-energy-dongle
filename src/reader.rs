use std::collections::VecDeque;
use std::pin::Pin;
use std::task::{Context, Poll, ready};

use futures_util::Stream;

use crate::Bytes;

/// Raw bytes of a single DSMR telegram exposed in the public `contents` field.
///
/// Instances of [RawTelegram] produced by [RawTelegramStream] are guaranteed to contain only bytes of a single telegram. This
/// includes the header ("/ID") and footer with CRC and terminating CRLF ("/CRC\r\n").
///
/// [RawTelegram] implements `AsRef<[u8]>` for a convenient usage as a byte slice.
#[derive(Debug)]
pub struct RawTelegram {
	pub contents: Vec<u8>,
}

impl AsRef<[u8]> for RawTelegram {
	fn as_ref(&self) -> &[u8] {
		&self.contents
	}
}

/// Buffered DSMR telegram extractor from the partial byte buffers.
///
/// By repeatedly calling [RawTelegramReader::feed] with new bytes, the extractor will return a `Vec` with all new complete
/// telegrams contained so far.
///
/// # Example
/// ```
///  let mut reader = homey_energy_dongle::reader::RawTelegramReader::new();
///  let telegrams = reader.feed(b"DDDD");
///  assert!(telegrams.is_empty());
///  let telegrams = reader.feed(b"\r\n/");
///  assert!(telegrams.is_empty());
///  let telegrams = reader.feed(b"test\r\n!AAAA");
///  assert!(telegrams.is_empty());
///  let telegrams = reader.feed(b"\r\nDDDD\r\n/test2");
///  assert_eq!(1, telegrams.len());
/// ```
#[derive(Default)]
pub struct RawTelegramReader {
	partial_telegram: Vec<u8>,
}

impl RawTelegramReader {
	/// Creates a new [RawTelegramReader] instance.
	pub fn new() -> Self {
		RawTelegramReader {
			partial_telegram: vec![],
		}
	}

	/// Add new bytes to the internal buffer and return a `Vec` of all found complete DSMR telegrams.
	///
	/// After a telegram is extracted, its bytes are removed from the internal buffer, so the same telegram will not be produced
	/// twice.
	pub fn feed(&mut self, bytes: &[u8]) -> Vec<RawTelegram> {
		let mut out = vec![];

		let mut telegram_bytes = if self.partial_telegram.is_empty() {
			bytes
		} else {
			self.partial_telegram.extend_from_slice(bytes);
			self.partial_telegram.as_slice()
		};

		let rest = loop {
			let (telegram, rest) = extract_telegram(telegram_bytes);
			if let Some(telegram) = telegram {
				out.push(RawTelegram {
					contents: telegram.to_vec(),
				});
				telegram_bytes = rest;
			} else {
				break rest;
			}
		};

		if self.partial_telegram.is_empty() {
			self.partial_telegram.extend_from_slice(&bytes[bytes.len() - rest.len()..]);
		} else {
			self.partial_telegram.drain(..self.partial_telegram.len() - rest.len());
		}
		out
	}
}

/// Wrapper that converts a [Stream] of [Bytes] into a [Stream] of [RawTelegram].
///
/// Can be used in conjunction with [crate::websocket::WebsocketEnergyDongle] to convert separate [Bytes] buffers into parsable
/// DSMR telegrams.
///
/// See the [crate-level documentation](crate) for more details and examples.
pub struct RawTelegramStream<S> {
	reader: RawTelegramReader,
	ready_telegrams: VecDeque<RawTelegram>,
	inner: S,
}

impl<S: Stream<Item = Bytes>> RawTelegramStream<S> {
	pub fn new(inner: S) -> Self {
		RawTelegramStream {
			reader: RawTelegramReader::new(),
			ready_telegrams: VecDeque::new(),
			inner,
		}
	}
}

impl<S: Stream<Item = Bytes> + Unpin> Stream for RawTelegramStream<S> {
	type Item = RawTelegram;

	fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
		if let Some(first_ready_telegram) = self.ready_telegrams.pop_front() {
			return Poll::Ready(Some(first_ready_telegram));
		}
		let out = loop {
			let Some(bytes) = ready!(Pin::new(&mut self.inner).poll_next(cx)) else {
				return Poll::Ready(None);
			};
			let telegrams = self.reader.feed(&bytes);
			if !telegrams.is_empty() {
				let mut telegrams = telegrams.into_iter();
				let out = telegrams.next();
				self.ready_telegrams.extend(telegrams);
				break out;
			}
		};
		Poll::Ready(out)
	}
}

fn extract_telegram(bytes: &[u8]) -> (Option<&[u8]>, &[u8]) {
	const TELEGRAM_START: u8 = b'/';
	const TELEGRAM_END: u8 = b'!';
	const CRLF: &[u8] = b"\r\n";

	let Some(telegram_start) = find_line_starting_with(bytes, TELEGRAM_START).and_then(|offset| bytes.get(offset..)) else {
		return (None, &[]);
	};
	let Some(last_line_offset) = find_line_starting_with(telegram_start, TELEGRAM_END) else {
		return (None, telegram_start);
	};
	let Some(last_line) = telegram_start.get(last_line_offset..) else {
		return (None, telegram_start);
	};
	let Some(last_line_end) = find_subslice(last_line, CRLF) else {
		return (None, telegram_start);
	};

	let telegram_end_offset = last_line_offset + last_line_end + CRLF.len();

	let res = telegram_start.split_at_checked(telegram_end_offset);
	(res.map(|(telegram, _)| telegram), res.map_or(&[], |(_, rest)| rest))
}

fn find_line_starting_with(bytes: &[u8], start: u8) -> Option<usize> {
	let start_line = [b'\n', start];
	let start_line = start_line.as_slice();
	if bytes.starts_with(&[start]) {
		Some(0)
	} else {
		find_subslice(bytes, start_line).map(|offset| offset + start_line.len() - 1)
	}
}

fn find_subslice<T>(haystack: &[T], needle: &[T]) -> Option<usize>
where
	for<'a> &'a [T]: PartialEq,
{
	if needle.is_empty() {
		return None;
	}
	haystack.windows(needle.len()).position(|window| window == needle)
}

#[cfg(test)]
mod tests {
	use super::RawTelegramReader;

	#[test]
	fn test_telegram_reader() {
		{
			let mut reader = RawTelegramReader::new();
			let telegrams = reader.feed(b"/test\r\n!AAAA\r\n");
			assert_eq!(1, telegrams.len());
		}

		{
			let mut reader = RawTelegramReader::new();
			let telegrams = reader.feed(b"DDDD\r\n/test\r\n!AAAA\r\nADDDD");
			assert_eq!(1, telegrams.len());
		}

		{
			let mut reader = RawTelegramReader::new();
			let telegrams = reader.feed(b"DDDD\r\n/test\r\n!AAAA\r\nDDDD\r\n/test2");
			assert_eq!(1, telegrams.len());
		}

		{
			let mut reader = RawTelegramReader::new();
			let telegrams = reader.feed(b"DDDD");
			assert!(telegrams.is_empty());
			let telegrams = reader.feed(b"\r\n/");
			assert!(telegrams.is_empty());
			let telegrams = reader.feed(b"test\r\n!AAAA");
			assert!(telegrams.is_empty());
			let telegrams = reader.feed(b"\r\nDDDD\r\n/test2");
			assert_eq!(1, telegrams.len());
		}
	}
}
