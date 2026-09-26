# Bring existing skills under Dalo

Start with the inventory and preserve the user's intended scope. Migration can
be partial: uncertain content can remain where it is while known items move.
Distinguish two outcomes before acting: preserving today's installed content as
a local snapshot, or reconnecting selected skills to a verified upstream source
for future updates. A local snapshot does not receive upstream updates.

## Choose the destination from evidence

| Existing content | Default recommendation |
| --- | --- |
| User-authored, modified, or unknown real directory | Preserve its full contents in the local source, or intentionally keep it unmanaged |
| Verified unmodified third-party install with a Git origin | Reconnect through a catalog, selecting only the intended skills |
| Verified team repository the user trusts | Reuse or add the scoped team source |
| Foreign symlink | Inspect its canonical content and all consumers before choosing a handover |
| Project-only skill | Keep project scope; explain any target/store limitation before changing scope |
| Same name with different content | Preserve both; obtain the intended precedence or naming choice |

When provenance or the original revision cannot be recovered, keep the installed
content. Do not silently substitute the current upstream version. HTTP-only,
package-based, or other non-Git origins are not automatically Git sources.

## Adopt real directories

Dalo discovers adoptable directories only inside already configured targets.
Use an exact selector from `resolve list`, preferably the path when names repeat:

```sh
dalo --store "$store" --dry-run --json adopt "$skill_path"
dalo --store "$store" --json adopt "$skill_path"
dalo --store "$store" --dry-run --json adopt "$skill_path" --replace
dalo --store "$store" --json adopt "$skill_path" --replace
```

The first write copies into the local source and leaves the original in place.
The second replaces that original with a Dalo-owned link only when the existing
local copy still matches. Use the replacement when management of this exact
directory is within the migration request; a request merely to inspect or back
up a skill is not such a request. Adoption does not make a Git commit.

If the destination already exists with different content, preserve both and
resolve that choice. Do not delete either copy to make adoption pass. Invalid
slot names or frontmatter need a deliberate rename/edit, not a guessed mass
normalization. Use `resolve keep` for a real unmanaged directory the user wants
to retain. A copied-but-unreplaced skill can still conflict; report that state.

## Reconnect an upstream install

Use installer records as candidate provenance, then verify the repository,
skill path, and complete installed content. Inspect the candidate source and
compare before removing or replacing any existing entry. Adding a catalog pins
what Dalo resolves at that time; it does not import a skills CLI lock or restore
an arbitrary old ref. If the installed revision is unavailable or differs,
offer preservation as a local snapshot or an explicit update with a reviewed
diff. Do not copy foreign hash values into Dalo's lockfiles.

Reuse or add the catalog, select the exact skills, review their content and
audit results, and grant only the approval covered by the installation request.
Keep modified versions local, with an explicit precedence decision if a catalog
offers the same name. Do not both adopt and select an upstream copy without
explaining which version would win. Preview synchronization before handover.

## Handover of foreign entries

`adopt` and `resolve keep` do not handle foreign symlinks. `sync` will not take
them over, and `resolve remove-owned` is not a foreign-link removal command.

When the user requested full migration of specific foreign entries, prepare a
bounded, reversible handover using filesystem tools:

1. Record every affected path, raw link destination, canonical content location,
   and other discovered consumers. Check for Dalo ownership drift first. Stop
   that item if ownership or shared consumers are ambiguous.
2. Preserve the complete content in a unique recovery directory outside agent
   discovery folders and Dalo-managed checkouts. Record original paths and
   installer metadata there, preserving file modes and links. Verify the copy;
   retain both the original location information and recoverable content. A
   backup of only a symlink is not a backup of its content.
3. Prepare the selected Dalo source and inspect its audit and synchronization
   plan. For a local snapshot that cannot be adopted directly, copy the verified
   content into a new, unoccupied local skill directory; do not add ownership
   records yourself. If internal links would break after relocation, resolve
   that before switching. Do not overwrite an existing local skill.
4. Recheck the original entries and prepared content immediately before switching.
   If they changed, reassess. Move only the exact agreed unmanaged entries into
   the recovery directory without following them; never use a wildcard or remove
   a whole skill root. Shared canonical directories and dependent links must be
   treated as one handover so no remaining consumer is stranded.
5. Run Dalo synchronization, inspect each new link and its resources, and check
   every known shared consumer. If the intended links cannot be established,
   remove only newly created Dalo-owned links through Dalo and restore the saved
   entries into vacant original paths. If restoration would overwrite concurrent
   content, stop that item and report the exact recovery paths.

For a real directory being switched to a catalog, the same recovery process
applies. Prefer Dalo's adoption transaction when the intended destination is
the local source. Do not perform a manual handover during a bare invocation.

## Leave a usable outcome

Foreign installer records do not confer Dalo trust and do not transfer ownership.
Do not rewrite their schemas or automatically run `skills remove`/`skills update`
against paths now managed by Dalo. Report the old records and remaining external
installs; any cleanup must be scoped to those exact entries and preserve other
skills. Until reconciled, the old installer may still try to update migrated
paths. Keep recovery copies and explain which manager should be used for each
set. Include the assistant itself if it was bootstrapped through another manager;
do not strand or erase the skill currently guiding the operation.

Verify all migrated entries and report any that stayed external, conflicted,
or could only be preserved as snapshots. Never describe a partial migration as
complete solely because `sync` exited successfully.
