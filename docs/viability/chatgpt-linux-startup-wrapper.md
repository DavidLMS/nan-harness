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

Before upload, `chatgpt-wave12-stage.py` validates the canonical report with
the desktop checker and validates the reducer facts with its lane-local closed
validator. It then writes a separate envelope with exactly these top-level
fields: `diagnosticVersion`, `kind`, `wrapperSha256`, `observation`, and
`report`.

The envelope is diagnostic evidence only. It must not be passed to
`nanh-desktop-check validate-report`, submitted as a public report, or used to
claim native startup success. The workflow uploads only staged envelopes; the
private facts root and any process output are never uploaded.
