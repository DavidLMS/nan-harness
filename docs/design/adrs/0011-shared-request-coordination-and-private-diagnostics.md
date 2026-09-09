# Shared request coordination and private diagnostics

## Scope and capacity

The managed bridge shares coordinator capacity by provider and credential across
launches. Models do not create independent quota pools. Inference starts with
two permits and learns capacity up to ten; a foreground inference 429 halves
the window, with a floor of one. Healthy successes reset the rejection streak
and retain the existing evidence and growth-hold rules. Control and background
failures do not penalize foreground inference capacity or set its cooldown.

## Rate-limit recovery

Without a valid `Retry-After`, consecutive foreground 429 observations select
random delays in these inclusive ranges:

| Rejection | Delay |
| --- | --- |
| First | 15–20 seconds |
| Second | 30–40 seconds |
| Third and later | 45–60 seconds |

The coordinator pauses the shared scope and preserves any longer existing
cooldown. Control/background rejections use the first range without changing
the foreground streak. An uncoordinated bridge, or an unavailable observation reply, uses the
same local 429 policy, indexed by attempt within its retry loop. Managed
capacity-acquisition failures retain their existing error boundary. Other local
and coordinator failure backoffs remain unchanged.

A valid `Retry-After` takes precedence for 429s, including zero and HTTP dates.
Past dates mean zero. Fractional durations are rounded upward for the existing
millisecond IPC fields. Unrepresentable cooldown deadlines never expire in the
coordinator process; saturation must not turn an extreme hint into an early
retry. Invalid or overflowing delta-second headers use local policy.

The HTTP retry loop makes at most **three sends**. Responses semantic recovery
retains its existing total of at most **eight sends** across loops. The logical
request owns two cumulative retry-pause allowances: **120 seconds combined**,
and **45 seconds for all non-429 reasons**, including semantic recovery. A 429
never replenishes or enlarges the non-429 allowance. Healthy response time does
not consume either pause allowance.

Reserve a whole delay before retrying. If it cannot fit, return the original
provider response/error; do not shorten the hint. Still observe the rejection
so other requests honor the shared cooldown. Observe the final attempt too,
but never sleep after it. Semantic recovery retains the same budgets and may
only replay before generated content has reached the harness. Pre-content
stream waits retain progress keepalives and cancellation.

These allowances are **not a 120-second wall-clock request deadline**. Capacity
acquisition has its separate existing one-hour bridge queue allowance per
acquisition; concurrent requests can extend shared cooldowns while a request is
queued. Network response timeouts and stream inactivity limits are separate.
Model discovery retains its existing 45-second overall discovery budget.

## Private diagnostic runbook

Use the existing opt-in private diagnostic controls when troubleshooting.
`permit_granted` reports queue time and classification. `attempt_observed`
reports outcome, provider `retry_after_ms`, chosen numeric `retry_delay_ms`,
header timing and capacity changes. Its `retry_delay_source` is a closed value:
`provider_hint` when a hint determines the delay, or `local_policy` otherwise;
non-retryable outcomes have a null source. For server errors a larger local
backoff still wins over a smaller hint.

Compare queue time separately from chosen retry pauses and the number of sends.
A terminal 429 can legitimately have a chosen delay in diagnostics even though
this request cannot retry. That observation protects other requests. A longer
shared cooldown can also outlast the delay chosen for an individual observation.

No wire protocol changes or new payload fields are needed for this policy.
Diagnostic events remain private and opt-in; do not copy captures, prompts,
credentials, provider error text or model output into issues or repository
artifacts. This policy does not change token accounting, harness context limits
or Codex's documented 90% context behavior.
