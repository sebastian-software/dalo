// Illustrative portable handler: bound input, no native stdin interpretation.
const project = process.argv[2];
if (!project) throw new Error('Expected the bound project directory');
process.stdout.write(JSON.stringify({
  kind: 'add_context',
  context: `When reviewing UI changes in ${project}, check contrast and keyboard access.`,
}));
