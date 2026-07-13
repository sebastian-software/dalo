#!/bin/sh
set -eu

output=""
url=""
while [ "$#" -gt 0 ]; do
  case "$1" in
    -o)
      output="$2"
      shift 2
      ;;
    -*) shift ;;
    *)
      url="$1"
      shift
      ;;
  esac
done

if [ -z "$output" ] || [ -z "$url" ]; then
  echo "fake curl: expected a URL and -o output" >&2
  exit 1
fi

asset="${url##*/}"
cp "${DALO_INSTALLER_FIXTURES}/${asset}" "$output"
