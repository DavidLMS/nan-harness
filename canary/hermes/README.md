# Hermes private-profile qualification

`check.py` verifies the managed CLI profile for Hermes0.22.5 and newer. Hosted
deterministic conformance runs it before the published inventory/tool/sentinel
contracts. It uses the installed, frozen Hermes executable and loopback scripted
providers; it never receives the real NaN credential.

The fixture borrows only installed dependencies. Its synthetic native home has
unrelated configuration, history, control files and automation sentinels. All of
those files must remain byte-identical, with no new native files. A local IRC
listener also proves that inherited platform activation cannot start a bot.
Synthetic ancestor-service markers verify that the owned gateway identifies its
external supervisor independently of systemd, launchd or Desktop ancestry.

Two launches reopen the same multiword-titled conversation. The second must keep
the original session ID and first-turn context while using a fresh authenticated
bridge endpoint. The previous provider remains available and must receive no
further requests. Separate native launches check effort, disabled reasoning and
the MiMo thinking toggle on the actual provider request. Disabled MiMo reasoning
accepts either `reasoning_effort: "none"` (Hermes 0.23.2 custom routes) or
`chat_template_kwargs.enable_thinking: false` (the model-aware provider plugin),
as documented by [NaN](https://nan.builders/docs/models#controlling-reasoning).
Missing and contradictory controls fail; closed receipts distinguish absent
requests from mismatched controls without retaining request content.

The probe records only gateway identities under its own private profile. On
Windows it retains process handles before checking exit; on Unix it observes
process absence without signaling discovered PIDs. Fixture deletion requires
observed gateway exit for every launch. Uncertain cleanup retains the private
workspace and fails qualification. Only a bounded closed JSON receipt leaves the
probe; child output, prompts, responses and configuration remain private.

The stable managed CLI home is independent of the user's native Hermes home.
Native conversations are neither imported nor migrated. Rust owns the native
foreground gateway and keeps the NaN bridge alive until the gateway drains;
launch configuration is removed only after successful cleanup. History remains
available to subsequent managed launches.

Managed profiles copy the installed PM runtime selection and borrow its committed
runtime generations alongside application dependencies. Missing PM state must not
be confused with stale application dependencies: that starts an update during
chat startup. A native `update_paused` refusal is retained as the closed
`NH-HERMES-FRONTEND-UPDATE-PAUSED` diagnostic.

Hermes 0.23.5 and newer also include enabled plugins in the dependency
environment stamp. The managed profile intentionally enables different plugins
from the native home, so every native action names a private
`installs/launcher-state` folder through `HERMES_INSTALL_STATE_DIR`. Hermes then
uses the interpreter Rust resolved, and never selects, repairs or builds a
dependency environment. Lazy dependency installs are therefore refused during
managed launches instead of writing into the native installation. Cleanup
removes the folder with the staged installs; older releases ignore it.
