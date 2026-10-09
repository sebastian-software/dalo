# Pinned Next.js project discovery fixture

This offline fixture covers issue #954 using the directory symlink paths and
literal targets from `vercel/next.js` commit
`e273d5b2e869d0be06c0e86f788e1594f9dd40cf`. `symlinks.json` lists all 25 symlinks
that the full checkout reports as `skipped_symlink`, together with the real
terminal directories needed to recreate their resolution. Regular-file symlinks
are omitted because inventory already ignores them.

`project_install_restores_nextjs_pinned_symlink_layout` creates this layout in a
local Git repository. Skill bodies are synthetic, so the fixture does not run
or vendor upstream code. The test uses the issue's exact three Next.js skill
selectors, the React skill selector, and both Claude/Codex targets. Local fixture
commits replace the upstream pins so the ordinary suite never needs the network.

The source snapshot is available at
<https://github.com/vercel/next.js/tree/e273d5b2e869d0be06c0e86f788e1594f9dd40cf>.
