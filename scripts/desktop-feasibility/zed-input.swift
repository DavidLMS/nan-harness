// Disposable hosted-runner input transport. Never logs its input.
import CoreGraphics
import Foundation

func post(_ key: CGKeyCode, _ down: Bool, _ flags: CGEventFlags, _ text: String? = nil) {
    guard let event = CGEvent(keyboardEventSource: nil, virtualKey: key, keyDown: down) else { exit(3) }
    event.flags = flags
    if let text {
        let units = Array(text.utf16)
        units.withUnsafeBufferPointer { buffer in
            event.keyboardSetUnicodeString(stringLength: buffer.count, unicodeString: buffer.baseAddress!)
        }
    }
    event.post(tap: .cghidEventTap)
    Thread.sleep(forTimeInterval: 0.01)
}

let mode = CommandLine.arguments.count == 1 ? "type" : CommandLine.arguments.count == 2 ? CommandLine.arguments[1] : "invalid"
let payload = FileHandle.standardInput.readData(ofLength: 4097)
if mode == "type" {
    guard !payload.isEmpty, payload.count <= 4096,
          let text = String(data: payload, encoding: .utf8),
          text.unicodeScalars.allSatisfy({ $0.value >= 32 && $0.value <= 126 }) else { exit(2) }
    // Posting is not a delivery acknowledgement; the checker verifies clipboard.
    for scalar in text.unicodeScalars {
        post(0, true, [], String(scalar))
        post(0, false, [], String(scalar))
    }
} else {
    guard payload.isEmpty else { exit(2) }
    let key: CGKeyCode
    let modifiers: [(CGKeyCode, CGEventFlags)]
    switch mode {
    case "new-thread": key = 45; modifiers = [(59, .maskControl), (58, .maskAlternate)]
    case "select-all": key = 0; modifiers = [(55, .maskCommand)]
    case "copy-thread": key = 16; modifiers = [(59, .maskControl), (58, .maskAlternate)]
    case "paste": key = 9; modifiers = [(55, .maskCommand)]
    case "copy": key = 8; modifiers = [(55, .maskCommand)]
    case "right": key = 124; modifiers = []
    case "submit": key = 36; modifiers = []
    default: exit(2)
    }
    var flags: CGEventFlags = []
    for (modifier, flag) in modifiers {
        flags.insert(flag)
        post(modifier, true, flags)
    }
    post(key, true, flags)
    post(key, false, flags)
    for (modifier, flag) in modifiers.reversed() {
        flags.remove(flag)
        post(modifier, false, flags)
    }
}
