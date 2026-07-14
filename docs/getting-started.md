# Getting Started

This guide walks through Dalo's core loop without touching your real agent folders
first. Install Dalo first (see the [README installation instructions](../README.md#installation)),
make sure `git` is on `PATH`, and use Linux or macOS.

## 1. Create a sandbox store

```sh
export DALO_STORE="$(mktemp -d)/store"
target_dir="$(mktemp -d)/skills"

dalo init
dalo target link generic "$target_dir"
```

The store is Dalo's local database. It contains source checkouts, the local source, lockfiles, approvals, and target state.

The target is where Dalo materializes the resolved skill set. In this sandbox it is just a temporary folder.

## 2. Add a local skill

```sh
mkdir -p "$DALO_STORE/local/skills/review"
cat > "$DALO_STORE/local/skills/review/SKILL.md" <<'EOF'
# Review

Check behavioral regressions before style nits.
EOF
```

The local source is private to your machine. It is useful for experiments and overrides.

## 3. Sync

```sh
dalo status
dalo sync
ls -la "$target_dir"
```

`sync` refreshes clean tracking sources, resolves one approved skill set, and links that set into configured targets. Dalo links directories it owns and refuses to overwrite unmanaged files.

## 4. Try a local team source

This uses a local Git repository, so it works without network access.

```sh
TEAM_REPO="$(mktemp -d)/team-skills"
mkdir -p "$TEAM_REPO/skills/release-notes"
cat > "$TEAM_REPO/skills/release-notes/SKILL.md" <<'EOF'
# Release Notes

Summarize user-visible changes first.
EOF

git -C "$TEAM_REPO" init
git -C "$TEAM_REPO" add .
git -C "$TEAM_REPO" -c commit.gpgSign=false -c user.email=test@example.com -c user.name='Test User' commit -m initial

dalo source add company "$TEAM_REPO"
dalo source list
dalo sync
```

A source is a Git-backed collection of skills. Source priority decides conflicts: lower priority wins. A slot is the portable skill name Dalo links into target folders.

Team sources are trusted by default, so their skills can sync immediately. Use
a catalog source when you want to review and approve individual skills.

## 5. Try a catalog source

This local catalog demonstrates the selection and approval flow without network
access:

```sh
CATALOG_REPO="$(mktemp -d)/catalog-skills"
mkdir -p "$CATALOG_REPO/skills/review-helper"
cat > "$CATALOG_REPO/skills/review-helper/SKILL.md" <<'EOF'
# Review Helper

Check behavioral regressions before style nits.
EOF

git -C "$CATALOG_REPO" init
git -C "$CATALOG_REPO" add .
git -C "$CATALOG_REPO" -c commit.gpgSign=false -c user.email=test@example.com -c user.name='Test User' commit -m initial

dalo source add-catalog public "$CATALOG_REPO"
dalo source inspect public
dalo source select public review-helper
dalo status
```

Catalog skills are untrusted by default. After reviewing the pending skill,
`status` prints the exact narrow approval command. Grant only that skill, then
sync it:

```sh
dalo audit public:review-helper
# Optional semantic review. This may send skill contents to the configured provider:
dalo audit public:review-helper --agent auto
dalo approve skill public:review-helper
dalo sync
```

`source add`, `source select`, and `approve skill` run deterministic local
preflight checks. `sync` repeats them against the exact content about to be
linked and blocks unaccepted `high` or `critical` findings. To accept a known
risk for one exact content hash and finding set, provide a reason:

```sh
dalo audit public:review-helper --accept-risk "reviewed pinned upstream installer"
```

Changing any file, adding findings, or upgrading an audit or review engine
invalidates that acceptance.

Use `dalo approve list` to inspect local trust rules. Broader `source`, `author`,
and `org` approvals are available when that is the intended policy.

## 6. Move from sandbox to a real agent

Unset the sandbox variables when you are ready to use your real store and agent folder:

```sh
unset DALO_STORE
dalo init
dalo target detect
```

Then link one real target:

```sh
dalo target link codex
# or: dalo target link claude
# or: dalo target link openclaw
# or: dalo target link hermes
```

Add your team source and sync:

```sh
dalo source add company git@github.com:acme/agent-skills.git
dalo sync
dalo doctor
```

## 7. Adopt a local skill

If an agent created a useful unmanaged skill directly in its folder, inspect it first:

```sh
dalo status
```

Then copy it into Dalo's local source:

```sh
dalo adopt release-notes.local
```

Adoption prints a local security preflight before it copies or replaces the
unmanaged skill.

Replacing the original folder with a Dalo-owned symlink is a separate explicit step:

```sh
dalo adopt --replace release-notes.local
```

Dalo does not commit adopted work automatically.
