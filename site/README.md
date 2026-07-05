# Dalo Site Redeploy Checklist

Before redeploying `dalo.sh`, verify the static site against the shipped repo state:

- Version strings match `Cargo.toml`.
- Target paths match `src/target.rs` and the README target table.
- The roadmap only lists future work; shipped features belong in feature content.
- Footer links resolve on `main`, including `README.md`, docs, issues, and `LICENSE`.
- `site/sitemap.xml` `lastmod` matches the redeploy date.

Quick checks:

```sh
rg -n "v[0-9]+\\.[0-9]+\\.[0-9]+|~/.agents/skills|catalog|instruction" site/index.html
rg -n "version = " Cargo.toml
rg -n "Codex|OpenClaw" README.md src/target.rs site/index.html
```
