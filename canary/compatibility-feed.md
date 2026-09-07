# Compatibility feed schema and runbook

nan-harness ships compatibility evidence inside the binary and can refresh that
evidence from a published feed without replacing the binary. This document
describes the two published assets, what a feed may and may not change, and how
the canary produces them.

## Published assets

Both assets live on the `compatibility` release of the repository:

| Asset | Schema | Contents | Consumers |
| --- | --- | --- | --- |
| `compatibility.json` | 2 | CLI harness evidence only | releases built before the unified feed |
| `compatibility-v3.json` | 3 | the same CLI evidence plus Desktop evidence | current releases |

Both are generated from the same accepted evidence. The legacy asset must never
gain a field: its consumers parse it with `deny_unknown_fields`, so any unknown
key makes the whole feed unusable for them. Desktop evidence therefore exists
only in the unified asset.

Release builds point `NAN_COMPATIBILITY_MANIFEST_URL` at
`compatibility-v3.json`. The variable remains an override at build time and at
run time; a client pointed at a schema-v2 feed accepts it as CLI-only evidence
and keeps its embedded Desktop registry.

## Schema

The document below is also checked in as
[`fixtures/compatibility-v3.json`](fixtures/compatibility-v3.json). Both the
publisher and the client validate that exact file in their own test suites, so
the two sides cannot drift apart silently.

```json
{
  "schemaVersion": 3,
  "releases": [
    {
      "nanHarnessVersion": "0.1.0",
      "verifications": [
        {
          "id": "codex",
          "lastCompatibleVersion": "0.147.0",
          "compatibleAt": "2026-09-07T00:00:00Z",
          "lastLiveVerifiedVersion": "0.147.0",
          "liveVerifiedAt": "2026-09-07T00:00:00Z"
        }
      ],
      "desktopVerifications": [
        {
          "id": "chatgpt-desktop",
          "platform": "macos",
          "evidence": "live-verified",
          "lastCompatibleAppVersion": "26.831.21537",
          "lastCompatibleRuntimeVersion": "0.152.0",
          "compatibleAt": "2026-09-07T00:00:00Z"
        }
      ]
    }
  ]
}
```

- `nanHarnessVersion` selects a record for exactly one release. A client never
  reads a record published for a different release, newer or older.
- `verifications` keeps the schema-v2 shape unchanged.
- `desktopVerifications` is keyed by surface and platform. Each record describes
  the application and, when the surface bundles one, its runtime, together.
- `evidence` is `live-verified` or `contract-only`. `unavailable` is a property
  of the embedded registry and can never be published or overridden.
- Every timestamp is RFC 3339. The embedded registry records plain dates; the
  producer normalizes them to midnight UTC when publishing.

## What remote evidence may change

A feed refines what the running binary already certifies. It may update
verification dates, the `lastCompatible*` version bounds and the evidence
classification of a surface that is already available. It may not change
minimum versions, available platforms, transports, executable paths,
credentials or adapters: those stay embedded, so a feed can never widen the
supported matrix or reroute a launch.

A Desktop record is adopted **atomically** — its own application and runtime
bounds replace the current pair together, or the record is ignored. Bounds from
two records are never combined, so a published pair is always one that was
observed together. Adoption additionally requires:

- a timestamp that is not older than the record it replaces;
- for a record of the same evidence class, a pair that does not regress on
  either track, and either an advancing bound or a later timestamp;
- for a promotion from `contract-only` to `live-verified`, the exact verified
  pair, which is adopted even when it is lower than the placeholder bounds it
  replaces (some platforms carry deliberately open contract-only bounds), and
  which may share the date of the record it replaces.

`live-verified` is never traded for `contract-only`, whatever the date. A
`live-verified` record must name the application version that was run, and must
also carry the runtime bound when its surface bundles a runtime; a surface whose
launcher cannot detect an installed application version therefore stays
`contract-only`. Downloading the feed successfully is metadata transport, not a
live verification, and the doctor output says which of the two a record is.

A record is rejected when it names an unknown platform for a known surface,
certifies a surface the embedded registry marks unavailable, carries evidence
below an embedded minimum, claims live verification without the bounds above,
has a malformed timestamp, or duplicates a surface and platform.

Records published for a **different** nan-harness release are checked for shape
only. Their platforms and minimums belong to the binary that produced them, so
this binary neither applies them nor invalidates the feed's history over them.
A record whose surface identifier this binary does not recognize is decoded by
the typed JSON schema like any other and then ignored, without further checks,
so a feed written for a newer surface stays consumable.

Even a `live-verified` record only reports a tested version when the launcher
can read the installed one. A launcher that cannot detect an installed version
reports contract-only evidence regardless of what the feed says.

### What metadata cannot repair

Refreshed evidence moves version bounds and dates. It cannot repair a protocol
change in a Desktop application or its bundled runtime: when a surface changes
how it authenticates or speaks to the local bridge, the client needs new code,
which means a one-time nan-harness upgrade. Desktop clients released before the
unified feed also need that one-time upgrade to receive Desktop evidence at
all, because their embedded registry is not refreshable.

## Client behavior

- Refresh runs at startup at most once per hour, with a three-second bounded
  request, HTTPS (or loopback for tests), restricted redirects and a 1 MiB cap.
  It is skipped entirely for `--dry-run`, for `NAN_NO_COMPATIBILITY_CHECK`, and
  under `CI`.
- The cache lives in the private configuration directory as
  `compatibility-v3.json`, alongside — never overwriting — the schema-v2 cache
  an older binary may have written.
- Cached evidence is bound to the feed it came from by a SHA-256 fingerprint of
  the configured URL; the URL itself is never persisted, because it may carry a
  token. Changing `NAN_COMPATIBILITY_MANIFEST_URL` discards the previous cache
  instead of answering from it. The cache is written as a private file on every
  platform.
- An invalid download never replaces a known-good cache. A failed download
  keeps the valid cache; without one, the embedded evidence stays in effect and
  the failure is reported as a safe advisory diagnostic. A cache document this
  binary cannot read is replaced by one bounded download rather than blocking
  refresh forever.
- Desktop overlays are applied inside the registry lookup, so every Desktop
  launcher and `nanh doctor` observe the same effective record and the same
  effective source.

## Producing the assets

```sh
cargo xtask compatibility-feed <FILE>                          # schema v2
cargo xtask unified-compatibility-feed <FILE>                  # schema v3
cargo xtask merge-compatibility-feed <BASE> <DIR> <FILE>       # schema v2
cargo xtask merge-unified-compatibility-feed <BASE> <DIR> <FILE>
cargo xtask validate-compatibility-feed <FILE>
cargo xtask validate-unified-compatibility-feed <FILE>
```

An update directory holds one JSON file per accepted result. A CLI update names
a harness `id`; a Desktop update also names a `platform`. Both are shape-checked
by either merge, but only the unified feed carries the Desktop records: a
directory of Desktop-only results is a complete run for the unified asset and
republishes the legacy asset unchanged.

When the run updates the release this checkout builds, surfaces missing from
that release's record are filled from this checkout's registry, without
overwriting explicit updates. No other release is ever seeded: evidence from
this source cannot certify a different binary.

## Publishing

`canary/host/publish-compatibility.sh` publishes both assets under one
host lock, legacy asset first. Each asset is recovered, migrated, merged and
validated on its own, then staged, backed up, swapped and verified with the
same rollback contract described in the [canary runbook](README.md).
A release that has no unified asset yet inherits the history the legacy feed
already proved, at the unified schema.

Publishing the unified asset never invalidates the legacy one: it is a separate
asset, replaced after the legacy swap has already been verified. A failure
while publishing the unified asset leaves the legacy asset published and valid,
and the next run recovers the unified asset from its backup.
