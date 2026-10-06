# J-Jump development instructions

## Private development records

Keep requirements, design drafts, iteration Specs, approval notes, progress records and acceptance logs outside Git.
Configure their external directory with `git config --local j-jump.processRoot /absolute/private/records` or
`J_JUMP_PROCESS_ROOT`. Read those records before product work. Never copy them into this repository, including ignored
folders. A new checkout can run repository checks without private records; product delivery with changed scope requires
one active external Product claim with confirmed requirements and recorded approval. Record the proposal/claim before
implementation and keep exact commit identities in the external record. Terminal records are frozen except for authorized
corrections. Only user, installation, configuration, release-tool and runnable-test documentation belongs in Git.

## Delivery

Use main only. Preserve unrelated changes. Run `./scripts/install-hooks.sh`, applicable tests and
`./scripts/verify-workflow.sh --local --changed-since BASE` before delivery. Commit, push ordinary fast-forward main
updates and read back the exact remote SHA. Do not rewrite history, force-push, publish, change repository visibility or
run hosted workflows without explicit user authority. The only configured hosted workflow is guarded manual release;
automatic triggers stay disabled; manual execution and publication require their own explicit authority. Distinguish local tests, exact installed-package acceptance and publication.

VERSION is authoritative. Product changes increment one patch by default; process/documentation changes do not advance
it. Major/minor changes require explicit user authority. New versions need matching Cargo metadata.

## Product and data boundaries

The product is a Rust CLI with Bash/Zsh/Fish adapters and optional terminal pickers. Keep a single current implementation;
do not add old-format migrations or automatic fallback. Local navigation must work without a network or credentials.
Jev and Cloudflare Clef-Flash are optional semantic providers; suggestions always require explicit selection. Only the parent shell changes directory.
Never execute model output or unquoted paths as shell code. Never claim a target is supported from compilation alone.

Use synthetic profiles for tests. File profiles do not isolate the shared OS credential entry; credential tests need an
isolated backend/session and synthetic keys. Never read, overwrite or delete the user's credential entry during testing.
Do not track keys, private paths, conversations or host logs. Never send source contents, Git remotes, environment,
shell history or credentials to the provider. Preserve unrelated Cargo configuration, user state and licensing notices.

Use `./scripts/test-product.sh` for applicable runtime changes. Documentation/governance changes require repository unit
checks, workflow/link/disclosure validation and `git diff --check`; repeat installed-product tests only when their inputs change.
