import Foundation
import AppKit
import CoreGraphics
import ApplicationServices

let arguments = CommandLine.arguments
guard arguments.count >= 3, let processId = Int32(arguments[2]), processId > 0 else { exit(2) }
let windows = CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID) as? [[String: Any]] ?? []
let owned = windows.filter { window in
    guard let owner = window[kCGWindowOwnerPID as String] as? Int32,
          let layer = window[kCGWindowLayer as String] as? Int,
          let bounds = window[kCGWindowBounds as String] as? [String: CGFloat] else { return false }
    return owner == processId && layer == 0 && (bounds["Width"] ?? 0) >= 600 && (bounds["Height"] ?? 0) >= 400
}
guard owned.count == 1, let window = owned.first,
      let identifier = window[kCGWindowNumber as String] as? Int,
      let bounds = window[kCGWindowBounds as String] as? [String: CGFloat],
      let left = bounds["X"], let top = bounds["Y"], let width = bounds["Width"], let height = bounds["Height"] else { exit(1) }
if arguments[1] == "window" {
    let result: [String: Any] = ["id": identifier, "x": left, "y": top, "width": width, "height": height,
                               "accessibilityTrusted": AXIsProcessTrusted(), "screenCaptureAllowed": CGPreflightScreenCaptureAccess()]
    let data = try JSONSerialization.data(withJSONObject: result, options: [.sortedKeys])
    print(String(data: data, encoding: .utf8)!)
    exit(0)
}
guard arguments.count >= 5, let horizontal = Double(arguments[3]), let vertical = Double(arguments[4]),
      horizontal >= left, horizontal < left + width, vertical >= top, vertical < top + height,
      let application = NSRunningApplication(processIdentifier: processId) else { exit(2) }
application.activate(options: [.activateIgnoringOtherApps])
usleep(180_000)
let point = CGPoint(x: horizontal, y: vertical)
guard let moved = CGEvent(mouseEventSource: nil, mouseType: .mouseMoved, mouseCursorPosition: point, mouseButton: .left) else { exit(3) }
moved.post(tap: .cghidEventTap)
usleep(60_000)
if arguments[1] == "click" {
    guard let down = CGEvent(mouseEventSource: nil, mouseType: .leftMouseDown, mouseCursorPosition: point, mouseButton: .left),
          let up = CGEvent(mouseEventSource: nil, mouseType: .leftMouseUp, mouseCursorPosition: point, mouseButton: .left) else { exit(3) }
    down.post(tap: .cghidEventTap)
    usleep(80_000)
    up.post(tap: .cghidEventTap)
} else if arguments[1] == "scroll", arguments.count == 6, let amount = Int32(arguments[5]), (-600...600).contains(amount) {
    guard let event = CGEvent(scrollWheelEvent2Source: nil, units: .pixel, wheelCount: 1, wheel1: amount, wheel2: 0, wheel3: 0) else { exit(3) }
    event.location = point
    event.post(tap: .cghidEventTap)
} else { exit(2) }
