use std::collections::HashSet;
use std::net::{IpAddr, SocketAddr};
use std::pin::pin;
use std::time::Duration;

use async_timer::Timed;
use mdns_sd::{ServiceDaemon, ServiceEvent};

pub const ENERGY_DONGLE_SERVICE_TYPE: &str = "_energydongle._tcp.local.";

/// Perform mDNS discovery and return the Homey Energy Dongles found on the local network.
///
/// Due to the unpredictable nature of mDNS, you need to supply the `timeout` argument to specify how long should the call wait
/// for the DNS results. If you know how many dongles there are on the LAN, specify that value in `max_hosts` so that the function
/// returns as soon as the requested count is reached. Otherwise, specify `0` or [usize::MAX] to collect as many as possible
/// within the specified interval.
///
/// It is also possible that this call doesn't return any results even if dongles are available. For more robustness it's
/// recommended to check the result of the call and retry a couple of times if it's empty.
///
/// See the [crate-level documentation](crate) for more details and examples.
pub async fn discover_devices_with_mdns(timeout: Duration, max_hosts: usize) -> mdns_sd::Result<Vec<EnergyDongleHostInfo>> {
	let max_hosts = if max_hosts == 0 {
		usize::MAX
	} else {
		max_hosts
	};
	let mdns = ServiceDaemon::new()?;
	let receiver = mdns.browse(ENERGY_DONGLE_SERVICE_TYPE)?;
	let mut out = Vec::with_capacity(1);
	{
		let discover = async {
			while let Ok(event) = receiver.recv_async().await {
				if let ServiceEvent::ServiceResolved(info) = event {
					let svc = info.as_resolved_service();
					let mut path = None;
					let mut version = None;
					for txt in svc.txt_properties.iter() {
						match txt.key() {
							"p" => path = Some(txt.val_str().to_string()),
							"v" => version = Some(txt.val_str().to_string()),
							_ => {}
						}
					}
					if let Some((path, version)) = path.zip(version) {
						out.push(EnergyDongleHostInfo {
							name: svc.fullname,
							hostname: svc.host,
							addresses: svc.addresses,
							port: svc.port,
							path,
							version,
						});
						if out.len() >= max_hosts {
							break;
						}
					}
				}
			}
		};
		let discover = pin!(discover);
		// ignore result because timeout is the expected flow
		let _ = Timed::platform_new(discover, timeout).await;
	}
	mdns.shutdown()?;
	Ok(out)
}

/// Host information about a Homey Energy Dongle found using mDNS query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnergyDongleHostInfo {
	pub name: String,
	pub hostname: String,
	pub addresses: HashSet<IpAddr>,
	pub port: u16,
	pub path: String,
	pub version: String,
}

impl EnergyDongleHostInfo {
	/// Convenience method to return a [SocketAddr] with the correct port for each [IpAddr] in `addresses`.
	pub fn socket_addresses(&self) -> impl Iterator<Item = SocketAddr> {
		self.addresses.iter().map(|addr| SocketAddr::new(*addr, self.port))
	}
}
