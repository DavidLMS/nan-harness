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
[`examples/compatibility-v3.json`](examples/compatibility-v3.json). Both the
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

A feed refines what the running binary already certifies. It may update:

- verification dates;
- the upper `lastCompatible*` version bounds, and only upwards;
- the evidence classification of a surface that is already available.

It may not change minimum versions, available platforms, transports,
executable paths, credentials or adapters. Those stay embedded, so a feed can
never widen the supported matrix or reroute a launch. A record is rejected when
it names an unknown platform for a known surface, certifies a surface the
embedded registry marks unavailable, carries evidence below an embedded
minimum, has a malformed timestamp, or duplicates a surface and platform.

A `live-verified` Desktop record must carry every bound its surface tracks: a
runtime-only or application-only record cannot certify the pair. Downloading
the feed successfully is metadata transport, not a live verification, and the
doctor output says which of the two a record actually is.

Unknown surface identifiers are validated for shape and then ignored, so a feed
published for a newer release does not break an older client.

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
- Cached evidence is bound to the feed it came from. Changing
  `NAN_COMPATIBILITY_MANIFEST_URL` discards the previous cache instead of
  answering from it.
- An invalid download never replaces a known-good cache. A failed download
  keeps the valid cache; without one, the embedded evidence stays in effect and
  the failure is reported as a safe advisory diagnostic.
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
a harness `id`; a Desktop update also names a `platform` and is ignored by the
legacy merge, which stays CLI-only. Merging a release the run touched publishes
the embedded Desktop evidence for that release when the feed does not describe
it yet, so every registered surface receives an entry.

## Publishing

`canary/host/publish-compatibility.sh` publishes both assets under one
host lock, legacy asset first. Each asset is recovered, migrated, merged and
validated on its own, then staged, backed up, swapped and verified with the
same rollback contract described in the [canary runbook](../canary/README.md).
A release that has no unified asset yet inherits the history the legacy feed
already proved, at the unified schema.

Publishing the unified asset never invalidates the legacy one: it is a separate
asset, replaced after the legacy swap has already been verified. A failure
while publishing the unified asset leaves the legacy asset published and valid,
and the next run recovers the unified asset from its backup.
