// Compose the draft body before publication, keeping the generated notes intact.
// Owned markers make recovery after a failed publish idempotent.
import { readFileSync, writeFileSync } from 'node:fs';

const [curatedPath, draftPath, outputPath, ...extra] = process.argv.slice(2);
if (!curatedPath || !draftPath || !outputPath || extra.length) {
  throw new Error('Usage: node scripts/prepare-release-notes.mjs <curated> <draft> <output>');
}

const start = '<!-- dalo:curated-release-notes:start -->';
const end = '<!-- dalo:curated-release-notes:end -->';
const draft = readFileSync(draftPath, 'utf8');
let generated = draft;
if (draft.includes(start) || draft.includes(end)) {
  const boundary = `\n${end}\n\n---\n\n`;
  const boundaryIndex = draft.indexOf(boundary);
  if (!draft.startsWith(`${start}\n`) || boundaryIndex < 0
      || draft.indexOf(start, start.length) !== -1
      || draft.indexOf(end, draft.indexOf(end) + end.length) !== -1) {
    throw new Error('Malformed curated release notes markers; leave the draft unchanged');
  }
  generated = draft.slice(boundaryIndex + boundary.length);
}

let curated;
try {
  curated = readFileSync(curatedPath, 'utf8').trimEnd();
} catch (error) {
  if (error.code !== 'ENOENT') throw error;
  // Releases without curated notes keep their body, including manual edits.
  writeFileSync(outputPath, draft);
  process.exit(0);
}
if (!curated.trim() || curated.includes(start) || curated.includes(end)) {
  throw new Error('Curated release notes must be nonempty and contain no owned markers');
}
writeFileSync(outputPath, `${start}\n${curated}\n${end}\n\n---\n\n${generated}`);
