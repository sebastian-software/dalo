'use strict';

const test = require('node:test');
const assert = require('node:assert/strict');
const { expectedChecksum, targetFor, versionFromTag } = require('../lib/release');

test('maps supported Node platforms to release targets', () => {
  assert.equal(targetFor('darwin', 'arm64'), 'aarch64-apple-darwin');
  assert.equal(targetFor('linux', 'x64'), 'x86_64-unknown-linux-gnu');
  assert.throws(() => targetFor('win32', 'x64'), /unsupported platform/);
});

test('parses release tags and checksum files strictly', () => {
  assert.equal(versionFromTag('dalo-v0.6.0'), '0.6.0');
  assert.equal(versionFromTag('v0.6.0'), '0.6.0');
  assert.equal(expectedChecksum('a'.repeat(64) + '  dalo.tar.gz\n'), 'a'.repeat(64));
  assert.throws(() => expectedChecksum('not-a-checksum\n'), /malformed/);
});
