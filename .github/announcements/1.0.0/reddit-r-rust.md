# r/rust

r/rust does not care that a CLI exists; it cares how it is built, what it
refuses to promise, and whether the crate is a real library or a binary with a
`lib.rs`. So this post leads with the sandbox, states the library stance
plainly instead of hiding it, and treats the product pitch as two paragraphs of
context.

Read the subreddit rules before posting; project announcements are allowed but
are expected to be technical, and the weekly "what are you working on" thread
is the fallback if a standalone post is not welcome. Do not post it as a
marketing piece.

Numbers in this draft must be re-checked against `main` at the tag; see the
checklist in README.md.

## Title

<!-- title -->
Dalo 1.0, a Rust CLI for delivering AI agent skills to a team: Landlock and Seatbelt sandboxing, a written compatibility contract, and why the crate is not a library

## Body

<!-- body -->
Dalo manages the skill files that coding agents read. Skills live in Git, the
CLI resolves one approved set and links it into the folders each agent already
reads, and a deterministic local preflight blocks a sync until a human approves
what changed. 1.0 is out. The parts that may interest this sub are the
sandboxing, the stability contract, and the library question.

**Sandboxing.** Dalo normally executes nothing from a source; it links
directories. There is exactly one case where source-provided code runs during a
sync: a package that has to build a provider-specific artifact. That generator
runs inside an OS sandbox that every descendant inherits.

- Linux: Landlock, ABI v4, requested as a hard requirement. Writes are allowed
  only below the one delivery staging directory; TCP connect and bind are
  denied. If the domain is not fully enforced, the process is not started at
  all, so an older kernel refuses rather than silently degrading.
- macOS: Seatbelt through `sandbox-exec` with a generated profile. Same write
  boundary, all network denied.
- Anywhere else: no mechanism, so generated delivery is simply unavailable.

Beyond the sandbox the generator gets a cleared environment, an empty `PATH`,
no stdin, its own process group, a bounded runtime, and bounded output, and it
must be the exact approved staged executable, matched by contract hash. The
documented limits are equally explicit: it is not a read boundary, Landlock
network control is TCP only so UDP and UNIX sockets are not covered, and hooks
run outside it with the user's authority, which is why approving a hook is a
separate, hash-bound decision.

**The compatibility contract is written down and tiered.** Tier 1, stable for
all of 1.x: every documented command and flag, the five exit codes, the
top-level shape of every `--json` report, and every file Dalo persists (each
carries a version field and is rejected rather than guessed at). Fields are
added, never removed or retyped; consumers must ignore unknown fields and
tolerate unknown enum values. Tier 3, explicitly not promised: human-readable
output, the store-internal layout, provider projections, the draft package
spec, and the Rust API. Deprecations are announced a minor ahead with a runtime
warning, which is why the pre-1.0 shims were removed before the line froze:
nothing deprecated is carried for the life of the major.

**Why the crate is not a library.** `cargo add dalo` works and docs.rs renders
it, but the library API is not a semver contract and I say so on the box. The
crate exists so CLI handlers stay thin and behavior can be tested without
spawning the binary; modules, types, and signatures may change in a patch. The
supported integration surface is the CLI: `--json` plus exit codes, both tier
1. If someone has a real use case for a stable Rust API, the answer is an issue
and a deliberate, curated crate, not a public module that leaked into a release
and now cannot move. That is recorded as an ADR rather than left as folklore.

Other engineering notes:

- Single binary, no daemon. `unsafe_code` is forbidden at the crate level, and
  the lint levels live in the manifest rather than in scattered attributes.
- It drives your own `git` instead of embedding an implementation, so your Git
  configuration and credential helpers still apply, and hooks already present
  in a managed checkout are not disabled. The security page states that plainly
  rather than implying isolation.
- Six published targets: macOS and Linux, `x86_64` and `aarch64`, `gnu` and
  `musl`. Each archive ships a SHA-256 checksum and a Sigstore bundle.
- Around a thousand tests behind a line-coverage gate in CI, and the upgrade
  path is a test rather than a promise: the suite restores stores written by
  four older released binaries, runs what a user runs after upgrading, and
  asserts the resolved set is unchanged, no owned link moves, and the second
  sync is a no-op.
- MSRV is pinned in `Cargo.toml` and is the only place it is defined.
  Dual-licensed MIT or Apache-2.0. No telemetry.

What it does not do: it does not make third-party instructions safe, it has no
native Windows build in 1.x (WSL only), and it is not a skill directory.

- Release notes: https://dalo.sh/news/1-0.html
- Compatibility contract: https://dalo.sh/docs/compatibility.html
- Security overview, including the limits: https://dalo.sh/docs/security.html
- Repository: https://github.com/sebastian-software/dalo

Criticism of the sandbox design or the tiering is welcome; both are the parts I
would most like to be wrong about early.
