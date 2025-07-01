# homey-energy-dongle

## Documentation

See [full documentation](https://docs.rs/homey-energy-dongle)

## Usage

Add this to your Cargo.toml:
```
[dependencies]
homey-energy-dongle = "0.3.2"
```

![Maintenance](https://img.shields.io/badge/maintenance-passively--maintained-yellowgreen.svg)
[![Build Status](https://github.com/twistedfall/homey-energy-dongle/actions/workflows/homey-energy-dongle.yml/badge.svg)](https://github.com/twistedfall/homey-energy-dongle/actions/workflows/homey-energy-dongle.yml)
[![Documentation](https://docs.rs/homey-energy-dongle/badge.svg)](https://docs.rs/homey-energy-dongle)

## Homey Energy Dongle local API access

Unofficial async implementation of [Homey Energy Dongle] discovery and local API access. See the [support page] for details on
how to enable it in your dongle.

The crate includes the mDNS discovery with the `discover` feature and the local API access with the `websocket` feature. Both
features are disabled by default.

The general workflow with this crate is as follows:
1. Discover the present Homey Energy Dongles on the network using mDNS using [discover_devices_with_mdns()] or supply
   the static address.
2. Establish the WebSocket connection using [WebsocketEnergyDongle::connect()]. All necessary arguments for this call are
   returned by the [discover_devices_with_mdns()] function. If you store the connection details statically, you need to have
   the IP address, port and WebSocket URL path. The path is usually "/ws".
3. The [WebsocketEnergyDongle] struct implements [Stream] over [Bytes] buffers that the dongle sends. These buffers don't
   always contain a complete DSMR telegram, so you'll need some kind of buffer to store the intermediate bytes. For this you
   can use a more low-level [RawTelegramReader] with its [RawTelegramReader::feed()] method
   or a more easy-to-use [RawTelegramStream] wrapper which implements [Stream] over [RawTelegram].
4. Use a DSMR parsing library (e.g., [dsmr5](https://crates.io/crates/dsmr5)) to parse the [RawTelegram] and get a readable
   DSMR telegram.

## Example
```rust
use std::net::SocketAddr;
use std::time::Duration;
use futures_util::{stream, StreamExt};

async fn example() {
    let dongles = homey_energy_dongle::discover::discover_devices_with_mdns(Duration::from_secs(5), 0).await.unwrap();
    for dongle in dongles {
        let addr = dongle.socket_addresses().next().unwrap();
        let dongle_buffers = homey_energy_dongle::websocket::WebsocketEnergyDongle::connect(addr, &dongle.path)
            .await
            .unwrap()
            .flat_map(|res| stream::iter(res.ok()));
        let mut raw_telegram_reader = homey_energy_dongle::reader::RawTelegramStream::new(dongle_buffers);
        while let Some(raw_telegram) = raw_telegram_reader.next().await {
            dbg!(&raw_telegram);
        }
    }
}
```

[Homey Energy Dongle]: https://homey.app/en-nl/homey-energy-dongle/
[support page]: https://support.homey.app/hc/en-us/articles/18985951863452-Enabling-the-Homey-Energy-Dongle-s-Local-API
[discover_devices_with_mdns()]: discover::discover_devices_with_mdns
[WebsocketEnergyDongle::connect()]: websocket::WebsocketEnergyDongle::connect
[WebsocketEnergyDongle]: websocket::WebsocketEnergyDongle
[Stream]: futures_util::Stream
[RawTelegramReader]: reader::RawTelegramReader
[RawTelegramReader::feed()]: reader::RawTelegramReader::feed
[RawTelegramStream]: reader::RawTelegramStream
[RawTelegram]: reader::RawTelegram

License: MIT OR Apache-2.0
