# Managed Docker SearXNG backend

The runtime Docker backend is an explicit maintenance contract. Building a plan,
launching a normal harness session, resolving search configuration, and making a
search request do not invoke Docker.

The explicit setup, update, stop, and remove operations use the official
docker.io/searxng/searxng image pinned by an OCI digest. The owned container is
nanh-search-searxng and carries the io.nan-harness ownership labels. Its host
endpoint is published only as 127.0.0.1:<port>:8080; its storage and
configuration are private per-user directories below the nan-harness data root.

Every Docker inspection addresses one exact transaction name. A name collision
without the expected labels is reported as a foreign resource and is never
stopped, removed, or replaced. Updates stage a replacement under a transaction
name and restore the prior owned container when creation or startup fails.
Failed explicit operations retain a private recovery record for the next
maintenance attempt.

This contract has deterministic fake-executor coverage for command arguments,
image digest validation, status inspection without startup, foreign-resource
preservation, active-session refusal, and update rollback.
