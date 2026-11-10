import Foundation
import Network

/// Watches whether this iPhone is connected to a network, with iOS's own monitor of the network's state, which only
/// reads that state: SeedLock opens no connection at all. iOS lets no app see whether airplane mode is on, so SeedLock
/// goes by the networks: it works only while none is connected. A VPN that iOS keeps on counts as one, even in airplane
/// mode with nothing under it: SeedLock cannot tell what a VPN carries.
final class NetworkCheck {
    private let monitor = NWPathMonitor()
    private let queue = DispatchQueue(label: "SeedLock network state")

    /// Starts watching: `changed` gets, on the screen's thread, why the iPhone is online (empty while it is offline),
    /// at once and then at every change.
    func start(_ changed: @escaping ([String]) -> Void) {
        monitor.pathUpdateHandler = { path in
            let reasons = NetworkCheck.reasons(path)
            DispatchQueue.main.async {
                changed(reasons)
            }
        }
        monitor.start(queue: queue)
    }

    /// Why the iPhone is online, as the monitor sees it now.
    var reasonsNow: [String] {
        NetworkCheck.reasons(monitor.currentPath)
    }

    /// Why the iPhone is online on this path; empty when it is offline.
    static func reasons(_ path: NWPath) -> [String] {
        var reasons: [String] = []
        switch path.status {
        case .satisfied, .requiresConnection:
            let vpn = throughVPN(path)
            if path.usesInterfaceType(.wifi) {
                reasons.append("It is connected to Wi-Fi.")
            }
            if path.usesInterfaceType(.cellular) {
                reasons.append("Mobile data is on.")
            }
            if path.usesInterfaceType(.wiredEthernet) {
                reasons.append("It is connected to a network by cable.")
            }
            if vpn {
                reasons.append("A VPN is on: turn it off, in Settings or in the VPN's own app (airplane mode does not "
                    + "turn it off).")
            } else if path.usesInterfaceType(.other) {
                reasons.append("It is connected to another network (a VPN, or a computer, for example).")
            }
            if path.status == .requiresConnection && !vpn {
                reasons.append("It connects to a network as soon as an app asks (a VPN that connects on demand, for "
                    + "example: turn it off, in Settings or in the VPN's own app).")
            } else if reasons.isEmpty {
                reasons.append("It is connected to a network.")
            }
        case .unsatisfied:
            // online, but not for SeedLock: other apps still are
            if path.unsatisfiedReason == .cellularDenied {
                reasons.append("Mobile data is on (it is off only for SeedLock, in Settings).")
            } else if path.unsatisfiedReason == .wifiDenied {
                reasons.append("It is connected to Wi-Fi (Wi-Fi is off only for SeedLock, in Settings).")
            }
        @unknown default:
            reasons.append("iOS describes the network in a way SeedLock does not know.")
        }
        return reasons
    }

    /// Whether the path goes through a VPN, whose own network iOS names utun, ipsec or ppp (with a number). Only for
    /// what SeedLock says: with or without a VPN, the iPhone is online.
    private static func throughVPN(_ path: NWPath) -> Bool {
        path.availableInterfaces.contains { interface in
            interface.type == .other && ["utun", "ipsec", "ppp"].contains { interface.name.hasPrefix($0) }
        }
    }
}
