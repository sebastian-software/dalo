// Test-only, explicitly authored bridge, not automatic native-plugin import.
// Only context-producing hooks are admitted. Control responses must never be
// silently downgraded to advisory context by a package integration.
import { readFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = dirname(fileURLToPath(import.meta.url));
const [mode, entry] = process.argv.slice(2);
const input = readFileSync(0, 'utf8');
const event = JSON.parse(input);
let stdout;
if (mode === 'replay') {
  const recording = JSON.parse(readFileSync(join(root, entry), 'utf8'));
  if (recording.exit !== 0 || recording.signal !== null) throw new Error('Failed recording');
  stdout = recording.stdout;
} else {
  const executable = mode === 'node' ? process.execPath
    : mode === 'text' ? '/bin/bash' : join(root, entry);
  const args = mode === 'engine' ? ['hook'] : [join(root, entry)];
  const result = spawnSync(executable, args, {
    input,
    cwd: event.cwd,
    // No inherited developer credentials, provider settings or launcher lookup.
    env: {
      PATH: '/usr/bin:/bin',
      HOME: event.cwd,
      XDG_CACHE_HOME: join(event.cwd, '.cache'),
      IMPECCABLE_HOOK_HARNESS: 'claude',
      IMPECCABLE_SKILL_DIR: root,
      IMPECCABLE_SELF: 'impeccable',
    },
    encoding: 'utf8',
    timeout: 10000,
    maxBuffer: 1024 * 1024,
  });
  if (result.error || result.status !== 0) {
    throw new Error(`Upstream hook failed: ${result.error ?? result.stderr}`);
  }
  stdout = result.stdout;
}

let context = stdout.trim();
if (mode !== 'text' && context) {
  const output = JSON.parse(stdout);
  if (Object.keys(output).length === 0) context = '';
  else {
    const specific = output.hookSpecificOutput;
    if (Object.keys(output).some(key => key !== 'hookSpecificOutput')
      || !specific || specific.hookEventName !== event.hook_event_name
      || Object.keys(specific).some(key => !['hookEventName', 'additionalContext'].includes(key))
      || typeof specific.additionalContext !== 'string') {
      throw new Error('Unsupported native output: expected context for this event only');
    }
    context = specific.additionalContext;
  }
}
process.stdout.write(JSON.stringify(context
  ? { kind: 'add_context', context }
  : { kind: 'abstain' }));
