# Getting Started

This guide walks through Dalo's core loop without touching your real agent folders first.

## 1. Create a sandbox store

```sh
export DALO_STORE="$(mktemp -d)/store"
export DALO_TARGET="$(mktemp -d)/skills"

dalo init
dalo target link generic "$DALO_TARGET"
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
ls -la "$DALO_TARGET"
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
git -C "$TEAM_REPO" -c user.email=test@example.com -c user.name='Test User' commit -m initial

dalo source add company "$TEAM_REPO"
dalo source list
dalo sync
```

A source is a Git-backed collection of skills. Source priority decides conflicts: lower priority wins. A slot is the portable skill name Dalo links into target folders.

If a catalog skill is pending review, `status` prints the exact narrow approval
command. After reviewing it, grant only that skill:

```sh
dalo approve skill public:review-helper
dalo sync
```

Use `dalo approve list` to inspect local trust rules. Broader `source`, `author`,
and `org` approvals are available when that is the intended policy.

## 5. Move from sandbox to a real agent

Unset the sandbox variables when you are ready to use your real store and agent folder:

```sh
unset DALO_STORE DALO_TARGET
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

## 6. Adopt a local skill

If an agent created a useful unmanaged skill directly in its folder, inspect it first:

```sh
dalo status
```

Then copy it into Dalo's local source:

```sh
dalo adopt release-notes.local
```

Replacing the original folder with a Dalo-owned symlink is a separate explicit step:

```sh
dalo adopt --replace release-notes.local
```

Dalo does not commit adopted work automatically.
