// Disposable hosted-runner Unicode input transport. Never logs its input.
import CoreGraphics
import Foundation

let payload = FileHandle.standardInput.readData(ofLength: 4097)
guard CommandLine.arguments.count == 1,
      !payload.isEmpty, payload.count <= 4096,
      let text = String(data: payload, encoding: .utf8),
      text.unicodeScalars.allSatisfy({ $0.value >= 32 && $0.value <= 126 }) else {
    exit(2)
}

// Use explicit neutral flags rather than inheriting asynchronous modifier state.
// Event posting is not a delivery acknowledgement; the checker verifies clipboard.
for scalar in text.unicodeScalars {
    let units = Array(String(scalar).utf16)
    for down in [true, false] {
        guard let event = CGEvent(keyboardEventSource: nil, virtualKey: 0, keyDown: down) else {
            exit(3)
        }
        event.flags = []
        units.withUnsafeBufferPointer { buffer in
            event.keyboardSetUnicodeString(stringLength: buffer.count, unicodeString: buffer.baseAddress!)
        }
        event.post(tap: .cghidEventTap)
    }
}
