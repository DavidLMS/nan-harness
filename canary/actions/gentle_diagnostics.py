"""Closed probe diagnostics; never derive a report code from process output."""
CODES = frozenset({
    "missing-live-key", "live-launch", "child-result", "child-read-evidence",
    "live-gateway-accounting", "live-child-routing", "provider-drain-timeout",
    "provider-auth", "provider-quota", "provider-throttled", "provider-http",
    "provider-transport", "provider-stream-incomplete", "probe-timeout",
    "probe-unavailable", "probe-contract",
})


class ProbeError(AssertionError):
    def __init__(self, code):
        if code not in CODES:
            raise ValueError("unknown Gentle Shell diagnostic")
        self.code = code
        super().__init__(code)


def require(condition, code):
    if not condition:
        raise ProbeError(code)
