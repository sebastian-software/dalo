import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';

const workflow = readFileSync(
  new URL('../.github/workflows/pr-ci-supersession.yml', import.meta.url),
  'utf8',
);
const script = workflow.split('          script: |\n')[1];
assert.ok(script, 'the workflow must define a cancellation script');
const runController = Object.getPrototypeOf(async function () {}).constructor(
  'github', 'context', 'core', 'setTimeout', script,
);
const repository = { owner: 'owner', repo: 'repo' };
const source = {
  repo: { id: 10, owner: { login: 'fork' } },
  ref: 'feature',
  sha: 'current',
};
const pull = {
  number: 5, state: 'open', created_at: '2026-11-21T10:00:00Z',
  head: source, base: { ref: 'main' },
};
const run = (number, sha, overrides = {}) => ({
  id: number + 100, run_number: number, head_sha: sha,
  head_branch: source.ref, head_repository: { id: source.repo.id },
  event: 'pull_request', status: 'in_progress',
  created_at: '2026-11-21T10:02:00Z', pull_requests: [], ...overrides,
});

async function cancelRuns(runs, { otherPulls = [], recheck = pull } = {}) {
  const cancelled = [];
  let reads = 0;
  const github = {
    rest: {
      pulls: {
        get: async params => {
          assert.deepEqual(params, { ...repository, pull_number: pull.number });
          return { data: reads++ ? recheck : pull };
        },
        list: params => {
          assert.equal(params.head, 'fork:feature');
          assert.equal(params.state, 'all');
          return otherPulls;
        },
      },
      actions: {
        listWorkflowRuns: params => {
          assert.deepEqual(params, {
            ...repository, workflow_id: 'ci.yml', event: 'pull_request',
            branch: source.ref, created: `>=${pull.created_at}`, per_page: 100,
          });
          return runs;
        },
        cancelWorkflowRun: async ({ owner, repo, run_id }) => {
          assert.deepEqual({ owner, repo }, repository);
          cancelled.push(run_id);
        },
      },
    },
    paginate: async (method, params) => method(params),
  };
  await runController(
    github, { repo: repository, payload: { number: pull.number } }, { info() {} },
    callback => callback(),
  );
  return cancelled;
}

test('cancels only queued or running CI superseded by a newer run for the same PR', async () => {
  const cancelled = await cancelRuns([
    run(1, 'older'),
    run(2, 'previous', { status: 'queued' }),
    run(3, 'current'),
    run(4, 'delayed'),
    run(0, 'current'),
    run(0, 'already-done', { status: 'completed' }),
    run(0, 'another-pr', { pull_requests: [{ number: 6 }] }),
    run(0, 'another-branch', { head_branch: 'other-feature' }),
    run(0, 'another-fork', { head_repository: { id: 11 } }),
    run(0, 'main', { event: 'push' }),
  ]);
  assert.deepEqual(cancelled, [102, 101]);
});

test('leaves runs alone when the PR head changes during discovery', async () => {
  const recheck = { ...pull, head: { ...source, sha: 'new-head' } };
  assert.deepEqual(await cancelRuns([run(2, 'older'), run(3, 'current')], { recheck }), []);
});

test('leaves runs alone until GitHub registers a replacement', async () => {
  assert.deepEqual(await cancelRuns([run(1, 'older'), run(2, 'previous')]), []);
});

test('skips ambiguous runs from multiple PRs on the same source branch', async () => {
  const otherPulls = [{ ...pull, number: 6, base: { ref: 'next' } }];
  const association = { pull_requests: [{ number: 5 }] };
  const cancelled = await cancelRuns([
    run(1, 'ambiguous'),
    run(2, 'confirmed', association),
    run(3, 'current', association),
  ], { otherPulls });
  assert.deepEqual(cancelled, [102]);
});

test('runs on first attempts of PR head updates only', () => {
  assert.match(workflow, /^  pull_request_target:\n    types: \[synchronize\]$/m);
  assert.match(workflow, /^    if: \$\{\{ github\.run_attempt == 1 \}\}$/m);
});
