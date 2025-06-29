use std::iter;
use std::net::SocketAddr;
use std::time::Duration;

use futures_util::{StreamExt, stream};
use homey_energy_dongle::discover::discover_devices_with_mdns;
use homey_energy_dongle::reader::RawTelegramStream;
use homey_energy_dongle::websocket::WebsocketEnergyDongle;

#[tokio::test]
async fn test_discover() {
	let mut retries = iter::repeat_n((), 2);
	let dongles = loop {
		let res = discover_devices_with_mdns(Duration::from_secs(5), 1).await.unwrap();
		if !res.is_empty() || retries.next().is_none() {
			break res;
		}
	};
	assert!(!dongles.is_empty());
	for dongle in dongles {
		let addr = dongle.socket_addresses().find(SocketAddr::is_ipv4).unwrap();
		let d = WebsocketEnergyDongle::connect(addr, &dongle.path)
			.await
			.unwrap()
			.flat_map(|res| stream::iter(res.ok()));

		let reader = RawTelegramStream::new(d).take(5);
		let count = reader.count().await;
		assert_eq!(5, count);
	}
}
