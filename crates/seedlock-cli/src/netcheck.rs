//! Is this computer connected to a network? SeedLock never uses the network itself: this only
//! looks at the computer's own routing table, to see whether there is a way out to other networks
//! (a "default route", which a computer has as soon as Wi-Fi or a cable connects it). Nothing is
//! sent or received. On Linux the table is read from /proc/net; on macOS and Windows the system's
//! `route` program is asked; on other systems SeedLock cannot tell.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// What SeedLock could find out.
pub enum NetState {
    /// Connected, through these network connections (interfaces).
    Online(Vec<String>),
    Offline,
    /// It could not be checked here; the text says why.
    Unknown(String),
}

/// Check now.
pub fn state() -> NetState {
    if cfg!(target_os = "linux") {
        // SEEDLOCK_TEST_NET_DIR lets the tests stand in their own routing tables.
        let dir = std::env::var_os("SEEDLOCK_TEST_NET_DIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/proc/net"));
        linux_state(&dir)
    } else if cfg!(target_os = "macos") {
        macos_state()
    } else if cfg!(target_os = "windows") {
        windows_state()
    } else {
        NetState::Unknown("SeedLock cannot check this on this system yet".to_string())
    }
}

fn add(found: &mut Vec<String>, interface: &str) {
    if !found.iter().any(|f| f == interface) {
        found.push(interface.to_string());
    }
}

fn linux_state(dir: &Path) -> NetState {
    let v4 = std::fs::read_to_string(dir.join("route"));
    let v6 = std::fs::read_to_string(dir.join("ipv6_route"));
    if v4.is_err() && v6.is_err() {
        return NetState::Unknown("the routing table could not be read".to_string());
    }
    let mut found = default_routes_v4(v4.as_deref().unwrap_or(""));
    for interface in default_routes_v6(v6.as_deref().unwrap_or("")) {
        add(&mut found, &interface);
    }
    if found.is_empty() { NetState::Offline } else { NetState::Online(found) }
}

/// The interfaces with an IPv4 default route, from /proc/net/route
/// (`Iface Destination Gateway Flags RefCnt Use Metric Mask ...`, hexadecimal fields).
fn default_routes_v4(table: &str) -> Vec<String> {
    const RTF_UP: u32 = 0x1;
    let mut found = Vec::new();
    for line in table.lines().skip(1) {
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.len() < 8 {
            continue;
        }
        let flags = u32::from_str_radix(f[3], 16).unwrap_or(0);
        if f[1] == "00000000" && f[7] == "00000000" && flags & RTF_UP != 0 && f[0] != "lo" {
            add(&mut found, f[0]);
        }
    }
    found
}

/// The interfaces with an IPv6 default route, from /proc/net/ipv6_route
/// (`destination length source length next-hop metric refcount use flags interface`).
/// The "unreachable" default that Linux keeps on `lo` does not count.
fn default_routes_v6(table: &str) -> Vec<String> {
    const RTF_UP: u32 = 0x0001;
    const RTF_REJECT: u32 = 0x0200;
    let mut found = Vec::new();
    for line in table.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.len() < 10 {
            continue;
        }
        let flags = u32::from_str_radix(f[8], 16).unwrap_or(0);
        if f[0] == "00000000000000000000000000000000" && f[1] == "00" && flags & RTF_UP != 0 && flags & RTF_REJECT == 0 && f[9] != "lo" {
            add(&mut found, f[9]);
        }
    }
    found
}

fn macos_state() -> NetState {
    let mut found = Vec::new();
    let mut asked = false;
    for (args, ipv6) in [(&["-n", "get", "default"][..], false), (&["-n", "get", "-inet6", "default"][..], true)] {
        let Ok(out) = Command::new("/sbin/route").args(args).stdin(Stdio::null()).stderr(Stdio::null()).output() else { continue };
        asked = true;
        if out.status.success() {
            if let Some(interface) = route_get_interface(&String::from_utf8_lossy(&out.stdout)) {
                // macOS keeps IPv6 routes on its own virtual interfaces (VPN tunnels, AirDrop) even offline
                let virtual_only = ["utun", "awdl", "llw", "ipsec"].iter().any(|v| interface.starts_with(v));
                if !(ipv6 && virtual_only) {
                    add(&mut found, &interface);
                }
            }
        }
    }
    if !asked {
        return NetState::Unknown("the system's route program could not be started".to_string());
    }
    if found.is_empty() { NetState::Offline } else { NetState::Online(found) }
}

/// Windows: the system's `route print`. Its headings and a few words are in the computer's language,
/// so only the routes themselves are read: they are numbers and addresses in every language.
fn windows_state() -> NetState {
    let route = std::env::var_os("SystemRoot")
        .map(|root| PathBuf::from(root).join("System32").join("route.exe"))
        .filter(|path| path.is_file())
        .unwrap_or_else(|| PathBuf::from("route"));
    let Ok(out) = Command::new(&route).arg("print").stdin(Stdio::null()).stderr(Stdio::null()).output() else {
        return NetState::Unknown("the system's route program could not be started".to_string());
    };
    if !out.status.success() {
        return NetState::Unknown("the system's route program did not answer".to_string());
    }
    let found = windows_default_routes(&String::from_utf8_lossy(&out.stdout));
    if found.is_empty() { NetState::Offline } else { NetState::Online(found) }
}

/// The connections that have a default route, in the output of Windows' `route print`:
/// - IPv4, an active route: `0.0.0.0  0.0.0.0  <gateway>  <interface address>  <metric>`. The gateway
///   can be words in the computer's language ("On-link", "En vínculo"), so the line is read from both
///   ends. Persistent routes (kept for when a connection comes back) have no interface address and
///   end in a metric or a word, so they do not count.
/// - IPv6: `<interface number>  <metric>  ::/0  <gateway>`, named from the interface list at the top.
fn windows_default_routes(output: &str) -> Vec<String> {
    let mut names = std::collections::HashMap::new();
    for line in output.lines() {
        // the interface list: " 12...00 15 5d 01 02 03 ......Intel(R) Wi-Fi 6 AX201"
        let trimmed = line.trim_start();
        let digits: String = trimmed.chars().take_while(|c| c.is_ascii_digit()).collect();
        if !digits.is_empty() && trimmed[digits.len()..].starts_with("...") {
            if let Some((_, name)) = trimmed.rsplit_once("..") {
                let name = name.trim_start_matches('.').trim();
                if !name.is_empty() {
                    names.insert(digits, name.to_string());
                }
            }
        }
    }
    let mut found = Vec::new();
    for line in output.lines() {
        let t: Vec<&str> = line.split_whitespace().collect();
        let n = t.len();
        if n >= 5 && t[0] == "0.0.0.0" && t[1] == "0.0.0.0" && t[n - 1].parse::<u32>().is_ok() {
            if is_ipv4_address(t[n - 2]) {
                add(&mut found, t[n - 2]);
            }
        }
        if n >= 4 && t[2] == "::/0" && t[0].parse::<u32>().is_ok() && t[1].parse::<u32>().is_ok() {
            let name = names.get(t[0]).cloned().unwrap_or_else(|| format!("IPv6 interface {}", t[0]));
            add(&mut found, &name);
        }
    }
    found
}

/// Is SeedLock running in a virtual machine? Its network adapter then always looks connected, even
/// while the real computer is offline, and nothing inside can see the real computer's connection.
/// Returns the virtual machine program's name, read from the firmware description the virtual
/// machine shows (no privileges needed, nothing sent): /sys/class/dmi/id on Linux, the registry on
/// Windows (`reg query`), sysctl on macOS. With SEEDLOCK_TEST_NET_DIR set, the tests' stand-in files
/// `sys_vendor` and `product_name` in that folder are read instead (none: not a virtual machine).
pub fn virtual_machine() -> Option<String> {
    let read_in = |dir: &Path, name: &str| std::fs::read_to_string(dir.join(name)).unwrap_or_default();
    if let Some(dir) = std::env::var_os("SEEDLOCK_TEST_NET_DIR") {
        let dir = PathBuf::from(dir);
        return vm_from_firmware(&read_in(&dir, "sys_vendor"), &read_in(&dir, "product_name"));
    }
    if cfg!(target_os = "linux") {
        let dir = Path::new("/sys/class/dmi/id");
        vm_from_firmware(&read_in(dir, "sys_vendor"), &read_in(dir, "product_name"))
    } else if cfg!(target_os = "windows") {
        let reg = std::env::var_os("SystemRoot")
            .map(|root| PathBuf::from(root).join("System32").join("reg.exe"))
            .filter(|path| path.is_file())
            .unwrap_or_else(|| PathBuf::from("reg"));
        let out = Command::new(reg)
            .args(["query", r"HKLM\HARDWARE\DESCRIPTION\System\BIOS"])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .ok()?;
        let text = String::from_utf8_lossy(&out.stdout);
        let value = |name: &str| registry_value(&text, name).unwrap_or_default();
        vm_from_firmware(&value("SystemManufacturer"), &value("SystemProductName"))
    } else if cfg!(target_os = "macos") {
        let out = Command::new("/usr/sbin/sysctl").args(["-n", "kern.hv_vmm_present"]).stdin(Stdio::null()).stderr(Stdio::null()).output().ok()?;
        (String::from_utf8_lossy(&out.stdout).trim() == "1").then(|| "a virtual machine program".to_string())
    } else {
        None
    }
}

/// The virtual machine program that a computer's firmware description names (its maker and
/// product), if it is one: the usual desktop ones, whose network adapter is the virtual machine's own.
fn vm_from_firmware(vendor: &str, product: &str) -> Option<String> {
    let (v, p) = (vendor.trim().to_lowercase(), product.trim().to_lowercase());
    let both = format!("{v} {p}");
    let name = if both.contains("qemu") || both.contains("kvm") {
        "QEMU/KVM"
    } else if both.contains("virtualbox") || v.contains("innotek") {
        "VirtualBox"
    } else if both.contains("vmware") {
        "VMware"
    } else if v.contains("microsoft") && p.contains("virtual machine") {
        "Hyper-V"
    } else if both.contains("parallels") {
        "Parallels"
    } else if v.contains("xen") || p.contains("hvm domu") {
        "Xen"
    } else if both.contains("bochs") {
        "Bochs"
    } else if both.contains("bhyve") {
        "bhyve"
    } else {
        return None;
    };
    Some(name.to_string())
}

/// A text value in the output of Windows' `reg query`: `    SystemManufacturer    REG_SZ    QEMU`.
fn registry_value(output: &str, name: &str) -> Option<String> {
    output.lines().find_map(|line| {
        let rest = line.trim_start().strip_prefix(name)?.trim_start();
        Some(rest.strip_prefix("REG_SZ")?.trim().to_string())
    })
}

/// Four numbers from 0 to 255 with dots between them, as `route print` writes an IPv4 address.
/// (Checked by hand: SeedLock's own code uses no networking module at all, as tools/audit-offline.sh checks.)
fn is_ipv4_address(text: &str) -> bool {
    let parts: Vec<&str> = text.split('.').collect();
    parts.len() == 4
        && parts.iter().all(|p| (1..=3).contains(&p.len()) && p.bytes().all(|b| b.is_ascii_digit()) && p.parse::<u16>().is_ok_and(|n| n <= 255))
}

/// The interface named in the output of `route -n get default` (`  interface: en0`).
fn route_get_interface(output: &str) -> Option<String> {
    output
        .lines()
        .find_map(|line| line.trim().strip_prefix("interface:").map(|i| i.trim().to_string()))
        .filter(|i| !i.is_empty() && !i.starts_with("lo"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROUTE_ONLINE: &str = "Iface\tDestination\tGateway \tFlags\tRefCnt\tUse\tMetric\tMask\t\tMTU\tWindow\tIRTT\n\
        wlan0\t00000000\t0101A8C0\t0003\t0\t0\t600\t00000000\t0\t0\t0\n\
        wlan0\t0001A8C0\t00000000\t0001\t0\t0\t600\t00FFFFFF\t0\t0\t0\n\
        docker0\t000011AC\t00000000\t0001\t0\t0\t0\t0000FFFF\t0\t0\t0\n";
    const ROUTE_OFFLINE: &str = "Iface\tDestination\tGateway \tFlags\tRefCnt\tUse\tMetric\tMask\t\tMTU\tWindow\tIRTT\n\
        docker0\t000011AC\t00000000\t0001\t0\t0\t0\t0000FFFF\t0\t0\t0\n";
    const V6_ONLINE: &str = "\
        00000000000000000000000000000000 00 00000000000000000000000000000000 00 fe800000000000000000000000000001 00000258 00000001 00000000 00450003     eth0\n\
        00000000000000000000000000000000 00 00000000000000000000000000000000 00 00000000000000000000000000000000 ffffffff 00000001 00000000 00200200       lo\n";
    const V6_OFFLINE: &str = "\
        fe800000000000000000000000000000 40 00000000000000000000000000000000 00 00000000000000000000000000000000 00000100 00000001 00000000 00000001     eth0\n\
        00000000000000000000000000000000 00 00000000000000000000000000000000 00 00000000000000000000000000000000 ffffffff 00000001 00000000 00200200       lo\n";

    #[test]
    fn a_default_route_means_online_and_local_routes_do_not() {
        assert_eq!(default_routes_v4(ROUTE_ONLINE), ["wlan0"]);
        assert!(default_routes_v4(ROUTE_OFFLINE).is_empty(), "a route to a local network (docker0) is not a way out");
        assert_eq!(default_routes_v6(V6_ONLINE), ["eth0"]);
        assert!(default_routes_v6(V6_OFFLINE).is_empty(), "the unreachable default on lo does not count");
    }

    #[test]
    fn the_state_comes_from_the_routing_tables() {
        let dir = std::env::temp_dir().join(format!("seedlock-netcheck-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("route"), ROUTE_OFFLINE).unwrap();
        std::fs::write(dir.join("ipv6_route"), V6_ONLINE).unwrap();
        assert!(matches!(linux_state(&dir), NetState::Online(i) if i == ["eth0"]), "an IPv6-only connection counts");
        std::fs::write(dir.join("ipv6_route"), V6_OFFLINE).unwrap();
        assert!(matches!(linux_state(&dir), NetState::Offline));
        std::fs::remove_dir_all(&dir).ok();
        assert!(matches!(linux_state(Path::new("/nonexistent/seedlock")), NetState::Unknown(_)));
    }

    const WINDOWS_ONLINE: &str = "\
===========================================================================
Interface List
 12...00 15 5d 01 02 03 ......Intel(R) Wi-Fi 6 AX201 160MHz
  1...........................Software Loopback Interface 1
===========================================================================

IPv4 Route Table
===========================================================================
Active Routes:
Network Destination        Netmask          Gateway       Interface  Metric
          0.0.0.0          0.0.0.0      192.168.1.1    192.168.1.100     35
        127.0.0.0        255.0.0.0         On-link         127.0.0.1    331
      192.168.1.0    255.255.255.0         On-link     192.168.1.100    291
===========================================================================
Persistent Routes:
  None

IPv6 Route Table
===========================================================================
Active Routes:
 If Metric Network Destination      Gateway
 12    291 ::/0                     fe80::1
  1    331 ::1/128                  On-link
===========================================================================
Persistent Routes:
  None
";

    #[test]
    fn reads_default_routes_from_windows_route_print_in_any_language() {
        assert_eq!(windows_default_routes(WINDOWS_ONLINE), ["192.168.1.100", "Intel(R) Wi-Fi 6 AX201 160MHz"]);
        // Spanish Windows: other headings, and "En vínculo" (two words) where English says On-link
        let spanish = "\
Lista de interfaces
  7...a4 bb 6d 11 22 33 ......Realtek PCIe GbE Family Controller
Tabla de rutas IPv4
Rutas activas:
Destino de red        Máscara de red   Puerta de enlace   Interfaz  Métrica
          0.0.0.0          0.0.0.0       En vínculo       10.8.0.2     26
        127.0.0.0        255.0.0.0       En vínculo      127.0.0.1    331
Rutas persistentes:
  Dirección de red  Máscara de red  Dirección de puerta de enlace  Métrica
          0.0.0.0          0.0.0.0      192.168.1.1  Predeterminada
";
        assert_eq!(windows_default_routes(spanish), ["10.8.0.2"], "a VPN's default route counts; the persistent one does not");
        // offline: only the computer's own routes, and a persistent default route waiting for a connection
        let offline = "\
IPv4 Route Table
Active Routes:
Network Destination        Netmask          Gateway       Interface  Metric
        127.0.0.0        255.0.0.0         On-link         127.0.0.1    331
Persistent Routes:
  Network Address          Netmask  Gateway Address  Metric
          0.0.0.0          0.0.0.0      192.168.1.1       1
IPv6 Route Table
Active Routes:
 If Metric Network Destination      Gateway
  1    331 ::1/128                  On-link
";
        assert!(windows_default_routes(offline).is_empty());
        // an IPv6-only connection counts, named from the interface list (or by its number)
        let ipv6_only = WINDOWS_ONLINE.replace("          0.0.0.0          0.0.0.0      192.168.1.1    192.168.1.100     35\n", "");
        assert_eq!(windows_default_routes(&ipv6_only), ["Intel(R) Wi-Fi 6 AX201 160MHz"]);
        assert_eq!(windows_default_routes(" 9    25 ::/0   fe80::1\n"), ["IPv6 interface 9"]);
    }

    #[test]
    fn recognises_ipv4_addresses_the_way_route_print_writes_them() {
        for good in ["192.168.1.100", "10.8.0.2", "0.0.0.0", "255.255.255.255"] {
            assert!(is_ipv4_address(good), "{good}");
        }
        for bad in ["256.1.1.1", "1.2.3", "1.2.3.4.5", "a.b.c.d", "1..2.3", "", "On-link", "vínculo", "1234.1.1.1"] {
            assert!(!is_ipv4_address(bad), "{bad}");
        }
    }

    #[test]
    fn recognises_a_virtual_machine_from_its_firmware_description() {
        for (vendor, product, vm) in [
            ("QEMU\n", "Standard PC (Q35 + ICH9, 2009)\n", "QEMU/KVM"),
            ("innotek GmbH", "VirtualBox", "VirtualBox"),
            ("VMware, Inc.", "VMware7,1", "VMware"),
            ("Microsoft Corporation", "Virtual Machine", "Hyper-V"),
            ("Parallels Software International Inc.", "Parallels Virtual Platform", "Parallels"),
            ("Xen", "HVM domU", "Xen"),
            ("Red Hat", "KVM", "QEMU/KVM"),
        ] {
            assert_eq!(vm_from_firmware(vendor, product).as_deref(), Some(vm), "{vendor} / {product}");
        }
        for (vendor, product) in [("Dell Inc.", "XPS 13 9310"), ("Microsoft Corporation", "Surface Pro 7"), ("LENOVO", "20XW0055GE"), ("", "")] {
            assert_eq!(vm_from_firmware(vendor, product), None, "{vendor} / {product} is a real computer");
        }
        let reg = "\r\nHKEY_LOCAL_MACHINE\\HARDWARE\\DESCRIPTION\\System\\BIOS\r\n    BiosMajorRelease    REG_DWORD    0x0\r\n    \
                   SystemManufacturer    REG_SZ    innotek GmbH\r\n    SystemProductName    REG_SZ    VirtualBox\r\n    SystemSKU    REG_SZ    \r\n";
        assert_eq!(registry_value(reg, "SystemManufacturer").as_deref(), Some("innotek GmbH"));
        assert_eq!(registry_value(reg, "SystemProductName").as_deref(), Some("VirtualBox"));
        assert_eq!(registry_value(reg, "BiosMajorRelease"), None, "only text values");
        assert_eq!(registry_value(reg, "System"), None, "a whole name, not the start of one");
    }

    #[test]
    fn reads_the_interface_that_macos_route_names() {
        let online = "   route to: default\ndestination: default\n       mask: default\n    gateway: 192.168.1.1\n  interface: en0\n      flags: <UP,GATEWAY,DONE,STATIC,PRCLONING>\n";
        assert_eq!(route_get_interface(online).as_deref(), Some("en0"));
        assert_eq!(route_get_interface(""), None, "no default route: route prints nothing on stdout");
        assert_eq!(route_get_interface("  interface: lo0\n"), None);
    }
}
