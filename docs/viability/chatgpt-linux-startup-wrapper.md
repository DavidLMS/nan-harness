# ChatGPT Linux startup wrapper diagnostic

This is a disposable Linux-only diagnostic for the existing ChatGPT Desktop
launch path. It is not a product qualification, a native `PASS`, or a change
to the public desktop report contract.

The wrapper observes only the exact deterministic ChatGPT launch and reduces
startup stderr in memory. The reducer writes owner-only, closed observation
facts containing bounded counts, grounded signature classifications, process
disposition, and measured identities. It never stores raw logs, prompts,
credentials, paths, or model output.

The workflow is manual, uses one Ubuntu 24.04 x64 job with a 60-minute bound,
read-only repository permissions, no provider credential, and no host policy,
setuid, sandbox, or privilege relaxation. It is intentionally not triggered by
pushes, so a branch push cannot accidentally spend the one optional run.

The runner emits a closed four-field diagnostic: `diagnosticVersion`, `kind`,
`wrapperSha256`, and `observation`; `observation` is the instrumented public
`Report`, not the facts document and not a five-field staged envelope. Before
upload, `chatgpt-wave12-stage.py` validates that embedded report by writing it
only to a private temporary snapshot and invoking the real desktop checker's
`validate-report` command. It separately validates the facts and requires
`wrapperSha256`, `facts.identity.shimSha256`, `facts.identity.reducerSha256`,
and `facts.identity.realNanhSha256` to match the actual wrapper, reducer, and
`nanh` files, including the embedded report's nanh identity. The staged
diagnostic retains those four fields and adds `startupFacts`, containing the
validated reducer facts for that probe. Thus the upload preserves the startup
classification and bounded counts alongside the instrumented report.

The envelope is diagnostic evidence only. It must not be passed to
`nanh-desktop-check validate-report`, submitted as a public report, or used to
claim native startup success. The workflow uploads only staged envelopes; the
private facts root and any process output are never uploaded.
