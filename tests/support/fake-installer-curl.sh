#!/bin/sh
set -eu

if [ -n "${DALO_FAKE_CURL_LOG:-}" ]; then
  printf '%s\n' "$*" >> "$DALO_FAKE_CURL_LOG"
fi

output=""
url=""
effective_url="false"
while [ "$#" -gt 0 ]; do
  case "$1" in
    -o)
      output="$2"
      shift 2
      ;;
    -w)
      effective_url="true"
      shift 2
      ;;
    -*) shift ;;
    *)
      url="$1"
      shift
      ;;
  esac
done

case "$url" in
  https://api.github.com/repos/*/releases/latest)
    if [ "${DALO_FAKE_LATEST_API_FAIL:-}" = "1" ]; then
      exit 22
    fi
    printf '{"tag_name":"dalo-v9.8.7"}\n'
    exit 0
    ;;
  https://github.com/*/releases/latest)
    if [ "$effective_url" = "true" ]; then
      printf 'https://github.com/sebastian-software/dalo/releases/tag/dalo-v9.8.7'
      exit 0
    fi
    ;;
esac

if [ -z "$output" ] || [ -z "$url" ]; then
  echo "fake curl: expected a URL and -o output" >&2
  exit 1
fi

asset="${url##*/}"
if [ "${DALO_FAKE_MISSING_BUNDLE:-}" = "1" ]; then
  case "$asset" in
    *.sigstore.json) exit 22 ;;
  esac
fi
cp "${DALO_INSTALLER_FIXTURES}/${asset}" "$output"
