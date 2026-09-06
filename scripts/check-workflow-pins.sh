#!/bin/sh
# Verify that every GitHub Actions `uses:` reference in the workflow directory
# names a full 40-character commit SHA and carries a trailing version comment.
#
# Floating references such as `@v5` or `@master` let a third party change the
# code that runs with this repository's release credentials. A SHA pin removes
# that; the comment keeps the pin readable and gives Renovate the version it
# needs to propose upgrades.
#
# Usage: sh scripts/check-workflow-pins.sh [workflow-directory]
set -eu

workflow_directory="${1:-.github/workflows}"

if [ ! -d "$workflow_directory" ]; then
  echo "workflow directory does not exist: $workflow_directory" >&2
  exit 2
fi

workflow_count=0
reference_count=0
failures=0

report() {
  echo "$1" >&2
  failures=$((failures + 1))
}

for workflow in "$workflow_directory"/*.yml "$workflow_directory"/*.yaml; do
  [ -f "$workflow" ] || continue
  workflow_count=$((workflow_count + 1))

  line_number=0
  while IFS= read -r line; do
    line_number=$((line_number + 1))

    # Only a `uses:` key counts. Strip indentation and an optional list dash so
    # that `run:` blocks mentioning the word are not mistaken for a reference.
    statement="$(printf '%s\n' "$line" | sed -e 's/^[[:space:]]*//' -e 's/^-[[:space:]]*//')"
    case "$statement" in
      uses:*) ;;
      *) continue ;;
    esac

    value="${statement#uses:}"
    comment=""
    case "$value" in
      *"#"*)
        comment="${value#*#}"
        value="${value%%#*}"
        ;;
    esac
    value="$(printf '%s\n' "$value" | sed -e 's/^[[:space:]]*//' -e 's/[[:space:]]*$//' -e 's/^["'\'']//' -e 's/["'\'']$//')"

    # Local composite actions and reusable workflows live in this repository and
    # are already pinned by the commit under test. Container actions name a
    # digest through their own image reference.
    case "$value" in
      ./*|.github/*|docker://*) continue ;;
    esac

    reference_count=$((reference_count + 1))

    revision=""
    case "$value" in
      *@*) revision="${value##*@}" ;;
    esac

    if [ -z "$revision" ]; then
      report "$workflow:$line_number: '$value' has no revision; pin it to a full commit SHA"
      continue
    fi

    pinned=1
    [ "${#revision}" -eq 40 ] || pinned=0
    case "$revision" in
      *[!0-9a-fA-F]*) pinned=0 ;;
    esac

    if [ "$pinned" -eq 0 ]; then
      report "$workflow:$line_number: '$value' is not pinned to a full 40-character commit SHA"
      continue
    fi

    # A pin without a version is unreadable and blocks automated upgrades.
    case "$comment" in
      *[0-9]*) ;;
      *) report "$workflow:$line_number: '$value' is missing a trailing version comment, for example '# v1.2.3'" ;;
    esac
  done < "$workflow"
done

if [ "$workflow_count" -eq 0 ]; then
  echo "no workflow files found in $workflow_directory" >&2
  exit 2
fi

if [ "$failures" -ne 0 ]; then
  echo "$failures unpinned or undocumented action reference(s); see the messages above" >&2
  exit 1
fi

echo "workflow pin checks passed ($reference_count references in $workflow_count workflows)"
