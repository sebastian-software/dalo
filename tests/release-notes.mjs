import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { copyFileSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { test } from 'node:test';

const script = fileURLToPath(new URL('../scripts/prepare-release-notes.mjs', import.meta.url));
const generated = '## 1.0.0\n\n* Keep `literal` $(text), links and Unicode: ü.\n\n';

function fixture(t) {
  const root = mkdtempSync(join(tmpdir(), 'dalo-release-notes-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const curated = join(root, 'curated.md');
  const draft = join(root, 'draft.md');
  const output = join(root, 'output.md');
  writeFileSync(draft, generated);
  return {
    curated, draft, output,
    run: () => spawnSync(process.execPath, [script, curated, draft, output], { encoding: 'utf8' }),
  };
}

test('prepend curated notes and preserve the exact generated body on recovery', (t) => {
  const f = fixture(t);
  writeFileSync(f.curated, '# Launch\n\nWhat changed.\n');
  assert.equal(f.run().status, 0);
  const first = readFileSync(f.output, 'utf8');
  assert.ok(first.includes('# Launch\n\nWhat changed.'));
  assert.ok(first.endsWith(`\n\n---\n\n${generated}`));
  writeFileSync(f.draft, first);
  assert.equal(f.run().status, 0);
  assert.equal(readFileSync(f.output, 'utf8'), first);
  writeFileSync(f.curated, '# Updated launch\n');
  assert.equal(f.run().status, 0);
  const updated = readFileSync(f.output, 'utf8');
  assert.ok(updated.includes('# Updated launch'));
  assert.ok(!updated.includes('What changed.'));
  assert.ok(updated.endsWith(generated));
});

test('a release without a curated file keeps its original body', (t) => {
  const f = fixture(t);
  assert.equal(f.run().status, 0);
  assert.equal(readFileSync(f.output, 'utf8'), generated);
});

test('ambiguous markers or invalid curated content block publication without writing', (t) => {
  const f = fixture(t);
  const start = '<!-- dalo:curated-release-notes:start -->';
  const end = '<!-- dalo:curated-release-notes:end -->';
  const malformed = [start, end, `prefix\n${start}\nx\n${end}\n\n---\n\n${generated}`,
    `${start}\n${start}\nx\n${end}\n\n---\n\n${generated}`];
  writeFileSync(f.curated, '# Launch');
  writeFileSync(f.output, 'leave existing output alone');
  for (const body of malformed) {
    writeFileSync(f.draft, body);
    assert.notEqual(f.run().status, 0);
    assert.equal(readFileSync(f.output, 'utf8'), 'leave existing output alone');
  }
  writeFileSync(f.draft, generated);
  for (const body of ['', '   \n', `# Launch\n${end}`]) {
    writeFileSync(f.curated, body);
    assert.notEqual(f.run().status, 0);
    assert.equal(readFileSync(f.output, 'utf8'), 'leave existing output alone');
  }
});

test('the real publish step stages notes before publishing and fails closed', (t) => {
  const root = mkdtempSync(join(tmpdir(), 'dalo-publish-step-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const workflow = readFileSync(new URL('../.github/workflows/publish.yml', import.meta.url), 'utf8');
  const job = workflow.split('  publish-github-release:\n')[1].split('\n  update-homebrew:')[0];
  const step = job.split('        run: |\n')[1].split('\n')
    .map((line) => line.replace(/^          /, '')).join('\n');
  assert.ok(step.includes('node scripts/prepare-release-notes.mjs'));
  assert.ok(step.includes('--draft=false'));
  const bin = join(root, 'bin');
  mkdirSync(bin);
  // Only the GitHub API boundary is faked. Execute the workflow's actual shell
  // and the actual composer against a local draft, with no credentials/network.
  writeFileSync(join(bin, 'gh'), `#!/bin/sh
set -eu
case "$*" in
  'release view dalo-v1.0.0 --json isDraft --jq .isDraft') printf 'true\\n' ;;
  'release view dalo-v1.0.0 --json assets --jq .assets[].name') cat "$PUBLISH_FIXTURE/assets" ;;
  'release view dalo-v1.0.0 --json body --jq .body') cat "$PUBLISH_FIXTURE/draft" ;;
  'release edit dalo-v1.0.0 --notes-file '*)
    test "$FAIL_NOTES_EDIT" = 0
    cp "$5" "$PUBLISH_FIXTURE/draft"
    printf 'notes\\n' >> "$PUBLISH_FIXTURE/events" ;;
  'release edit dalo-v1.0.0 --draft=false') printf 'publish\\n' >> "$PUBLISH_FIXTURE/events" ;;
  *) echo "unexpected gh call: $*" >&2; exit 1 ;;
esac
`, { mode: 0o755 });
  const assets = ['x86_64-unknown-linux-gnu', 'aarch64-unknown-linux-gnu',
    'x86_64-unknown-linux-musl', 'aarch64-unknown-linux-musl', 'aarch64-apple-darwin']
    .flatMap((target) => ['', '.sha256', '.sigstore.json'].map((suffix) => `dalo-1.0.0-${target}.tar.gz${suffix}`));
  for (const mode of ['curated', 'malformed', 'missing-asset', 'edit-failure', 'legacy']) {
    const cwd = join(root, mode);
    mkdirSync(cwd);
    writeFileSync(join(cwd, 'draft'), mode === 'malformed'
      ? '<!-- dalo:curated-release-notes:start -->\nbroken' : generated);
    writeFileSync(join(cwd, 'assets'), (mode === 'missing-asset' ? assets.slice(1) : assets).join('\n') + '\n');
    writeFileSync(join(cwd, 'events'), '');
    if (mode !== 'legacy') {
      mkdirSync(join(cwd, '.github/release-notes'), { recursive: true });
      writeFileSync(join(cwd, '.github/release-notes/1.0.0.md'), '# Curated launch\n');
      mkdirSync(join(cwd, 'scripts'));
      copyFileSync(script, join(cwd, 'scripts/prepare-release-notes.mjs'));
    }
    const result = spawnSync('bash', ['-e', '-o', 'pipefail', '-c', step], {
      cwd, encoding: 'utf8',
      env: { ...process.env, PATH: `${bin}:${process.env.PATH}`, PUBLISH_FIXTURE: cwd,
        RUNNER_TEMP: cwd, TAG_NAME: 'dalo-v1.0.0', FAIL_NOTES_EDIT: mode === 'edit-failure' ? '1' : '0' },
    });
    const events = readFileSync(join(cwd, 'events'), 'utf8');
    if (mode === 'curated') {
      assert.equal(result.status, 0, result.stderr);
      assert.equal(events, 'notes\npublish\n');
      assert.ok(readFileSync(join(cwd, 'draft'), 'utf8').endsWith(generated));
    } else if (mode === 'legacy') {
      assert.equal(result.status, 0, result.stderr);
      assert.equal(events, 'publish\n');
      assert.equal(readFileSync(join(cwd, 'draft'), 'utf8'), generated);
    } else {
      assert.notEqual(result.status, 0, mode);
      assert.equal(events, '', mode);
    }
  }
});
