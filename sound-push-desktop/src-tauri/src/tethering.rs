//! USB tethering (plan §8.1 "USB tethering routes PC internet through mobile data"). A phone
//! sharing its connection over USB appears as a network adapter: RNDIS or NCM for Android,
//! Apple Mobile Device Ethernet for iPhone. SoundPush works over it like over any network, but
//! when it also carries this computer's internet (the default route) that traffic uses the
//! phone's mobile data, so the UI warns and suggests USB through adb instead.

use std::net::IpAddr;

use serde::Serialize;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TetheringStatus {
    /// A phone's USB tethering adapter is up.
    pub active: bool,
    /// This computer's internet goes through it, using mobile data.
    pub internet_via_phone: bool,
    /// Connected devices reached through it (device ids).
    pub peers: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Adapter {
    pub usb_tethering: bool,
    /// A VPN's virtual adapter (plan §8.1 "VPN active on PC or phone").
    pub vpn: bool,
    /// What the adapter is called, for the VPN warning.
    pub name: String,
    /// Carries the default route.
    pub default_route: bool,
    /// Addresses with their on-link prefix length.
    pub networks: Vec<(IpAddr, u8)>,
}

/// A VPN carrying this computer's whole internet connection (plan §8.1 "VPN active on PC or
/// phone"). Only that is worth warning about: a VPN with a route of its own leaves the local
/// network alone, and so do mesh VPNs unless everything was routed through them.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VpnStatus {
    /// A VPN adapter is up and carries the default route.
    pub captures_internet: bool,
    /// What it is called, for the explanation.
    pub name: Option<String>,
}

/// Checks the adapters now; takes a moment, so call it off the UI thread.
pub fn vpn() -> VpnStatus {
    evaluate_vpn(&adapters())
}

pub fn evaluate_vpn(adapters: &[Adapter]) -> VpnStatus {
    match adapters.iter().find(|a| a.vpn && a.default_route) {
        Some(a) => VpnStatus {
            captures_internet: true,
            name: Some(a.name.clone()).filter(|n| !n.is_empty()),
        },
        None => VpnStatus::default(),
    }
}

/// Adapter names and descriptions of the VPN clients people actually run. The tunnel and PPP
/// interface types already cover Windows' built-in VPNs; these are the clients that install an
/// ordinary-looking Ethernet adapter instead, plus the usual interface names on Linux and macOS.
const VPN_NAMES: &[&str] = &[
    "tap-windows",
    "tap-nordvpn",
    "wireguard",
    "openvpn",
    "nordlynx",
    "mullvad",
    "protonvpn",
    "expressvpn",
    "surfshark",
    "cyberghost",
    "private internet access",
    "anyconnect",
    "pulse secure",
    "globalprotect",
    "forticlient",
    "sonicwall",
    "tailscale",
    "zerotier",
    "hamachi",
    "tun",
    "tap",
    "ppp",
    "utun",
    "ipsec",
    "wg",
];

/// Whether an adapter name or description belongs to a VPN. Short interface names match only at
/// the start and only when the rest is a number, so "Realtek PCIe GbE Family Controller" and
/// "Intel(R) Wi-Fi 6 AX201" are never mistaken for one.
fn looks_like_vpn(text: &str) -> bool {
    let text = text.trim().to_ascii_lowercase();
    VPN_NAMES.iter().any(|needle| match *needle {
        "tun" | "tap" | "ppp" | "utun" | "ipsec" | "wg" => {
            text.strip_prefix(needle).is_some_and(|rest| {
                !rest.is_empty()
                    && rest
                        .chars()
                        .all(|c| c.is_ascii_digit() || c == '-' || c == '_')
            })
        }
        _ => text.contains(needle),
    })
}
pub fn status(peers: &[(String, IpAddr)]) -> TetheringStatus {
    evaluate(&adapters(), peers)
}

pub fn evaluate(adapters: &[Adapter], peers: &[(String, IpAddr)]) -> TetheringStatus {
    let tethering: Vec<&Adapter> = adapters.iter().filter(|a| a.usb_tethering).collect();
    TetheringStatus {
        active: !tethering.is_empty(),
        internet_via_phone: tethering.iter().any(|a| a.default_route),
        peers: peers
            .iter()
            .filter(|(_, ip)| {
                tethering.iter().any(|a| {
                    a.networks
                        .iter()
                        .any(|(net, prefix)| same_network(*net, *ip, *prefix))
                })
            })
            .map(|(id, _)| id.clone())
            .collect(),
    }
}

fn same_network(a: IpAddr, b: IpAddr, prefix: u8) -> bool {
    match (a.to_canonical(), b.to_canonical()) {
        (IpAddr::V4(a), IpAddr::V4(b)) if (1..=32).contains(&prefix) => {
            let mask = u32::MAX << (32 - u32::from(prefix));
            u32::from(a) & mask == u32::from(b) & mask
        }
        (IpAddr::V6(a), IpAddr::V6(b)) if (1..=128).contains(&prefix) => {
            let mask = u128::MAX << (128 - u32::from(prefix));
            u128::from(a) & mask == u128::from(b) & mask
        }
        _ => false,
    }
}

/// Windows adapter descriptions of phone tethering drivers.
#[cfg_attr(not(windows), allow(dead_code))]
fn windows_description_is_tethering(description: &str) -> bool {
    let d = description.to_ascii_lowercase();
    [
        "remote ndis",
        "rndis",
        "usbncm",
        "ncm host",
        "apple mobile device ethernet",
    ]
    .iter()
    .any(|k| d.contains(k))
}

/// Linux: `rndis_host` (Android), `ipheth` (iPhone), `cdc_ncm` (newer Android, but also some
/// docks and USB network adapters, told apart by the USB product name).
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn linux_driver_is_tethering(driver: &str, product: &str) -> bool {
    match driver {
        "rndis_host" | "ipheth" => true,
        "cdc_ncm" => {
            let p = product.to_ascii_lowercase();
            !["dock", "hub", "ethernet", "lan", "adapter"]
                .iter()
                .any(|k| p.contains(k))
        }
        _ => false,
    }
}

/// The interface of the lowest-metric default route in `/proc/net/route`.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn default_route_interface(table: &str) -> Option<String> {
    table
        .lines()
        .skip(1)
        .filter_map(|line| {
            let f: Vec<&str> = line.split_whitespace().collect();
            // Iface Destination Gateway Flags RefCnt Use Metric Mask …; flag 0x1 = up.
            let up = u32::from_str_radix(f.get(3)?, 16).ok()? & 1 == 1;
            if !up || *f.get(1)? != "00000000" || *f.get(7)? != "00000000" {
                return None;
            }
            Some((f.get(6)?.parse::<u32>().ok()?, f[0].to_string()))
        })
        .min()
        .map(|(_, iface)| iface)
}

/// `networksetup -listallhardwareports` → (port name, device).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn parse_hardware_ports(text: &str) -> Vec<(String, String)> {
    let mut ports = Vec::new();
    let mut port = None;
    for line in text.lines() {
        if let Some(name) = line.strip_prefix("Hardware Port: ") {
            port = Some(name.trim().to_string());
        } else if let (Some(device), Some(name)) = (line.strip_prefix("Device: "), port.take()) {
            ports.push((name, device.trim().to_string()));
        }
    }
    ports
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn macos_port_is_tethering(port: &str) -> bool {
    let p = port.to_ascii_lowercase();
    p.contains("iphone usb") || p.contains("rndis") || p.contains("android")
}

/// Addresses in `ifconfig <device>` output.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn parse_ifconfig(text: &str) -> Vec<(IpAddr, u8)> {
    text.lines()
        .filter_map(|line| {
            let f: Vec<&str> = line.split_whitespace().collect();
            match f.as_slice() {
                ["inet", ip, "netmask", mask, ..] => {
                    let mask = u32::from_str_radix(mask.trim_start_matches("0x"), 16).ok()?;
                    Some((ip.parse().ok()?, mask.count_ones() as u8))
                }
                ["inet6", ip, rest @ ..] => {
                    let prefix = rest
                        .iter()
                        .position(|w| *w == "prefixlen")
                        .and_then(|i| rest.get(i + 1)?.parse().ok())?;
                    Some((ip.split('%').next()?.parse().ok()?, prefix))
                }
                _ => None,
            }
        })
        .collect()
}

#[cfg(windows)]
fn adapters() -> Vec<Adapter> {
    use std::net::{Ipv4Addr, Ipv6Addr};

    use windows::Win32::Foundation::{ERROR_BUFFER_OVERFLOW, NO_ERROR};
    use windows::Win32::NetworkManagement::IpHelper::{
        GAA_FLAG_INCLUDE_GATEWAYS, GAA_FLAG_SKIP_ANYCAST, GAA_FLAG_SKIP_DNS_SERVER,
        GAA_FLAG_SKIP_MULTICAST, GetAdaptersAddresses, GetBestInterfaceEx, IP_ADAPTER_ADDRESSES_LH,
    };
    use windows::Win32::NetworkManagement::Ndis::IfOperStatusUp;
    /// `IF_TYPE_PPP` and `IF_TYPE_TUNNEL` from the IANA interface types Windows reports.
    const IF_TYPE_PPP: u32 = 23;
    const IF_TYPE_TUNNEL: u32 = 131;
    use windows::Win32::Networking::WinSock::{
        AF_INET, AF_INET6, AF_UNSPEC, IN_ADDR, IN_ADDR_0, SOCKADDR, SOCKADDR_IN, SOCKADDR_IN6,
        SOCKET_ADDRESS,
    };

    /// SAFETY: `address` must come from GetAdaptersAddresses.
    unsafe fn socket_ip(address: &SOCKET_ADDRESS) -> Option<IpAddr> {
        // SAFETY: the pointer is null or points to a socket address of its family's size.
        unsafe {
            let family = address.lpSockaddr.as_ref()?.sa_family;
            if family == AF_INET {
                let v4 = &*address.lpSockaddr.cast::<SOCKADDR_IN>();
                Some(IpAddr::V4(Ipv4Addr::from(
                    v4.sin_addr.S_un.S_addr.to_ne_bytes(),
                )))
            } else if family == AF_INET6 {
                let v6 = &*address.lpSockaddr.cast::<SOCKADDR_IN6>();
                Some(IpAddr::V6(Ipv6Addr::from(v6.sin6_addr.u.Byte)))
            } else {
                None
            }
        }
    }

    let flags = GAA_FLAG_INCLUDE_GATEWAYS
        | GAA_FLAG_SKIP_ANYCAST
        | GAA_FLAG_SKIP_MULTICAST
        | GAA_FLAG_SKIP_DNS_SERVER;
    let mut size: u32 = 16 * 1024;
    let mut buffer: Vec<u64> = Vec::new();
    let mut filled = false;
    // The adapter list can grow between the size query and the call: retry a few times.
    for _ in 0..3 {
        buffer = vec![0u64; (size as usize).div_ceil(8)];
        // SAFETY: `buffer` holds at least `size` bytes, aligned for the adapter structs.
        let result = unsafe {
            GetAdaptersAddresses(
                u32::from(AF_UNSPEC.0),
                flags,
                None,
                Some(buffer.as_mut_ptr().cast()),
                &mut size,
            )
        };
        if result == NO_ERROR.0 {
            filled = true;
            break;
        }
        if result != ERROR_BUFFER_OVERFLOW.0 {
            break;
        }
    }
    if !filled {
        return Vec::new();
    }

    // The interface Windows would use for the internet (no packet is sent).
    let internet = SOCKADDR_IN {
        sin_family: AF_INET,
        sin_port: 0,
        sin_addr: IN_ADDR {
            S_un: IN_ADDR_0 {
                S_addr: u32::from_ne_bytes([1, 1, 1, 1]),
            },
        },
        sin_zero: [0; 8],
    };
    let mut best = 0u32;
    // SAFETY: a valid IPv4 socket address that the call only reads.
    let has_best =
        unsafe { GetBestInterfaceEx(std::ptr::from_ref(&internet).cast::<SOCKADDR>(), &mut best) }
            == NO_ERROR.0;

    let mut adapters = Vec::new();
    let mut next = buffer.as_ptr().cast::<IP_ADAPTER_ADDRESSES_LH>();
    // SAFETY: a linked list inside `buffer`, which outlives the loop.
    unsafe {
        while let Some(a) = next.as_ref() {
            next = a.Next.cast_const();
            if a.OperStatus != IfOperStatusUp || a.Description.is_null() {
                continue;
            }
            let description = a.Description.to_string().unwrap_or_default();
            let mut networks = Vec::new();
            let mut unicast = a.FirstUnicastAddress.cast_const();
            while let Some(u) = unicast.as_ref() {
                if let Some(ip) = socket_ip(&u.Address) {
                    networks.push((ip, u.OnLinkPrefixLength));
                }
                unicast = u.Next.cast_const();
            }
            // A friendly name ("NordLynx", "Ethernet 2") is what the user sees in Windows.
            let name = a
                .FriendlyName
                .to_string()
                .ok()
                .filter(|n| !n.is_empty())
                .unwrap_or_else(|| description.clone());
            adapters.push(Adapter {
                usb_tethering: windows_description_is_tethering(&description),
                // The tunnel and PPP interface types are Windows' own VPNs; a client that
                // installs an Ethernet-looking adapter is recognised by its name.
                vpn: matches!(a.IfType, IF_TYPE_PPP | IF_TYPE_TUNNEL)
                    || looks_like_vpn(&description)
                    || looks_like_vpn(&name),
                name,
                default_route: has_best
                    && a.Anonymous1.Anonymous.IfIndex == best
                    && !a.FirstGatewayAddress.is_null(),
                networks,
            });
        }
    }
    adapters
}

#[cfg(target_os = "linux")]
fn adapters() -> Vec<Adapter> {
    use std::collections::BTreeMap;
    use std::ffi::CStr;
    use std::net::{Ipv4Addr, Ipv6Addr};

    let default = std::fs::read_to_string("/proc/net/route")
        .ok()
        .and_then(|t| default_route_interface(&t));
    let mut by_name: BTreeMap<String, Vec<(IpAddr, u8)>> = BTreeMap::new();
    // SAFETY: the list from getifaddrs is only read, then freed once.
    unsafe {
        let mut list: *mut libc::ifaddrs = std::ptr::null_mut();
        if libc::getifaddrs(&mut list) != 0 {
            return Vec::new();
        }
        let mut next = list.cast_const();
        while let Some(ifa) = next.as_ref() {
            next = ifa.ifa_next.cast_const();
            if ifa.ifa_addr.is_null() || ifa.ifa_name.is_null() {
                continue;
            }
            let name = CStr::from_ptr(ifa.ifa_name).to_string_lossy().into_owned();
            let entry = match i32::from((*ifa.ifa_addr).sa_family) {
                libc::AF_INET => {
                    let addr = &*ifa.ifa_addr.cast::<libc::sockaddr_in>();
                    let prefix = ifa
                        .ifa_netmask
                        .cast::<libc::sockaddr_in>()
                        .as_ref()
                        .map_or(32, |m| m.sin_addr.s_addr.count_ones() as u8);
                    (
                        IpAddr::V4(Ipv4Addr::from(addr.sin_addr.s_addr.to_ne_bytes())),
                        prefix,
                    )
                }
                libc::AF_INET6 => {
                    let addr = &*ifa.ifa_addr.cast::<libc::sockaddr_in6>();
                    let prefix = ifa
                        .ifa_netmask
                        .cast::<libc::sockaddr_in6>()
                        .as_ref()
                        .map_or(128, |m| {
                            m.sin6_addr
                                .s6_addr
                                .iter()
                                .map(|b| b.count_ones())
                                .sum::<u32>() as u8
                        });
                    (IpAddr::V6(Ipv6Addr::from(addr.sin6_addr.s6_addr)), prefix)
                }
                _ => continue,
            };
            by_name.entry(name).or_default().push(entry);
        }
        libc::freeifaddrs(list);
    }
    by_name
        .into_iter()
        .map(|(name, networks)| {
            let device = std::path::Path::new("/sys/class/net")
                .join(&name)
                .join("device");
            let driver = std::fs::read_link(device.join("driver"))
                .ok()
                .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
                .unwrap_or_default();
            // `device` is the USB interface; the USB device with its product name is its parent.
            let product = std::fs::read_to_string(device.join("../product")).unwrap_or_default();
            Adapter {
                usb_tethering: linux_driver_is_tethering(&driver, &product),
                // A VPN interface has no hardware device behind it, and a telling name.
                vpn: looks_like_vpn(&name),
                default_route: default.as_deref() == Some(name.as_str()),
                name,
                networks,
            }
        })
        .collect()
}

/// Not yet run on macOS: parses `route`, `networksetup` and `ifconfig` output.
#[cfg(target_os = "macos")]
fn adapters() -> Vec<Adapter> {
    let run = |program: &str, args: &[&str]| {
        std::process::Command::new(program)
            .args(args)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
            .unwrap_or_default()
    };
    let default = run("/sbin/route", &["-n", "get", "default"])
        .lines()
        .find_map(|l| l.trim().strip_prefix("interface: ").map(str::to_string));
    let mut adapters: Vec<Adapter> =
        parse_hardware_ports(&run("/usr/sbin/networksetup", &["-listallhardwareports"]))
            .into_iter()
            .filter(|(port, _)| macos_port_is_tethering(port))
            .map(|(_, device)| Adapter {
                usb_tethering: true,
                vpn: false,
                default_route: default.as_deref() == Some(device.as_str()),
                networks: parse_ifconfig(&run("/sbin/ifconfig", &[&device])),
                name: device,
            })
            .collect();
    // VPNs have no hardware port, so the default route's own interface is checked by name
    // ("utun3", "ppp0"): that is the one that would capture the local network.
    if let Some(device) = default.filter(|d| looks_like_vpn(d)) {
        adapters.push(Adapter {
            usb_tethering: false,
            vpn: true,
            default_route: true,
            networks: parse_ifconfig(&run("/sbin/ifconfig", &[&device])),
            name: device,
        });
    }
    adapters
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
fn adapters() -> Vec<Adapter> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    #[test]
    fn peers_on_the_tethering_network_and_mobile_data_are_found() {
        let wifi = Adapter {
            usb_tethering: false,
            vpn: false,
            name: "Wi-Fi".into(),
            default_route: false,
            networks: vec![(ip("192.168.1.10"), 24)],
        };
        let phone = Adapter {
            usb_tethering: true,
            vpn: false,
            name: "Ethernet 3".into(),
            default_route: true,
            networks: vec![(ip("192.168.42.100"), 24), (ip("fe80::1"), 64)],
        };
        let peers = vec![
            ("pixel".to_string(), ip("192.168.42.129")),
            ("laptop".to_string(), ip("192.168.1.22")),
            ("mapped".to_string(), ip("::ffff:192.168.42.7")),
        ];
        let status = evaluate(&[wifi.clone(), phone.clone()], &peers);
        assert!(status.active && status.internet_via_phone);
        assert_eq!(status.peers, vec!["pixel", "mapped"]);

        let tethered_not_default = Adapter {
            default_route: false,
            ..phone
        };
        let status = evaluate(&[wifi.clone(), tethered_not_default], &peers);
        assert!(status.active && !status.internet_via_phone);
        assert_eq!(evaluate(&[wifi], &peers), TetheringStatus::default());
        assert!(!same_network(ip("10.0.0.1"), ip("192.168.0.1"), 0));
    }

    #[test]
    fn adapters_are_recognized_by_driver() {
        assert!(windows_description_is_tethering(
            "Remote NDIS based Internet Sharing Device #2"
        ));
        assert!(windows_description_is_tethering("UsbNcm Host Device"));
        assert!(windows_description_is_tethering(
            "Apple Mobile Device Ethernet"
        ));
        assert!(!windows_description_is_tethering(
            "Intel(R) Wi-Fi 6E AX211 160MHz"
        ));
        assert!(!windows_description_is_tethering(
            "Realtek USB GbE Family Controller"
        ));

        assert!(linux_driver_is_tethering("rndis_host", ""));
        assert!(linux_driver_is_tethering("cdc_ncm", "Pixel 8"));
        assert!(!linux_driver_is_tethering("cdc_ncm", "USB 10/100/1000 LAN"));
        assert!(!linux_driver_is_tethering("r8152", ""));

        assert!(macos_port_is_tethering("iPhone USB"));
        assert!(!macos_port_is_tethering("Wi-Fi"));
    }

    #[test]
    fn a_vpn_is_only_reported_when_it_carries_the_internet() {
        let adapter = |name: &str, vpn: bool, default_route: bool| Adapter {
            usb_tethering: false,
            vpn,
            name: name.into(),
            default_route,
            networks: vec![(ip("10.8.0.2"), 24)],
        };
        let wifi = adapter("Wi-Fi", false, true);
        let split_tunnel = adapter("Tailscale", true, false);
        assert_eq!(
            evaluate_vpn(&[wifi.clone(), split_tunnel.clone()]),
            VpnStatus::default(),
            "a VPN on a route of its own leaves the local network alone"
        );
        let full_tunnel = adapter("NordLynx", true, true);
        let status = evaluate_vpn(&[adapter("Wi-Fi", false, false), full_tunnel]);
        assert!(status.captures_internet);
        assert_eq!(status.name.as_deref(), Some("NordLynx"));
        assert_eq!(evaluate_vpn(&[wifi]), VpnStatus::default());
    }

    #[test]
    fn vpn_adapters_are_told_apart_from_ordinary_ones() {
        for name in [
            "TAP-Windows Adapter V9",
            "WireGuard Tunnel",
            "NordLynx",
            "Mullvad",
            "ProtonVPN TUN",
            "Cisco AnyConnect Secure Mobility Client Virtual Miniport Adapter",
            "tun0",
            "wg0",
            "utun3",
            "ppp0",
            "ipsec0",
            "tailscale0",
            "ZeroTier One [8056c2e21c000001]",
        ] {
            assert!(looks_like_vpn(name), "{name} is a VPN adapter");
        }
        for name in [
            "Intel(R) Wi-Fi 6E AX211 160MHz",
            "Realtek PCIe GbE Family Controller",
            "Ethernet 2",
            "eth0",
            "wlp3s0",
            "enp0s31f6",
            "Remote NDIS based Internet Sharing Device",
            "Hyper-V Virtual Ethernet Adapter",
            "",
        ] {
            assert!(!looks_like_vpn(name), "{name} is not a VPN adapter");
        }
    }

    #[test]
    fn os_tables_are_parsed() {
        let routes = "Iface\tDestination\tGateway \tFlags\tRefCnt\tUse\tMetric\tMask\t\tMTU\tWindow\tIRTT\n\
                      wlp2s0\t00000000\t0101A8C0\t0003\t0\t0\t600\t00000000\t0\t0\t0\n\
                      usb0\t00000000\t812AA8C0\t0003\t0\t0\t100\t00000000\t0\t0\t0\n\
                      usb0\t002AA8C0\t00000000\t0001\t0\t0\t100\t00FFFFFF\t0\t0\t0\n";
        assert_eq!(default_route_interface(routes).as_deref(), Some("usb0"));
        assert_eq!(default_route_interface("Iface\tDestination\n"), None);

        let ports = "Hardware Port: Wi-Fi\nDevice: en0\nEthernet Address: aa\n\n\
                     Hardware Port: iPhone USB\nDevice: en7\nEthernet Address: bb\n";
        assert_eq!(
            parse_hardware_ports(ports),
            vec![
                ("Wi-Fi".to_string(), "en0".to_string()),
                ("iPhone USB".to_string(), "en7".to_string())
            ]
        );

        let ifconfig = "en7: flags=8863<UP,BROADCAST> mtu 1500\n\
                        \tinet6 fe80::1c2b:3d4e%en7 prefixlen 64 scopeid 0x13\n\
                        \tinet 172.20.10.2 netmask 0xfffffff0 broadcast 172.20.10.15\n";
        assert_eq!(
            parse_ifconfig(ifconfig),
            vec![(ip("fe80::1c2b:3d4e"), 64), (ip("172.20.10.2"), 28)]
        );
    }
}
