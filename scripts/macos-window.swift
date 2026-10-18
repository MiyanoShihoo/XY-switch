import Foundation
import CoreGraphics

// Return only this process's normal, visible application window, not system chrome.
guard CommandLine.arguments.count == 2, let pid = Int32(CommandLine.arguments[1]) else { exit(2) }
let options: CGWindowListOption = [.optionOnScreenOnly, .excludeDesktopElements]
let windows = CGWindowListCopyWindowInfo(options, kCGNullWindowID) as? [[String: Any]] ?? []
for window in windows {
    guard let owner = window[kCGWindowOwnerPID as String] as? Int32, owner == pid,
          let layer = window[kCGWindowLayer as String] as? Int, layer == 0,
          let bounds = window[kCGWindowBounds as String] as? [String: CGFloat],
          (bounds["Width"] ?? 0) >= 600, (bounds["Height"] ?? 0) >= 400,
          let identifier = window[kCGWindowNumber as String] as? Int else { continue }
    print(identifier)
    exit(0)
}
exit(1)
