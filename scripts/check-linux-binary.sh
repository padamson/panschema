#!/usr/bin/env bash
# Check that Linux release binaries run where their archive says they do:
# a musl build links nothing at load time (no interpreter, no shared
# libraries), and a glibc build needs no glibc newer than GLIBC_FLOOR.
# Running a binary on the machine that built it proves neither, since that
# machine has every library and the newest glibc the binary could want.
# Each binary then runs `--version`, where the host can execute it.
#
# Usage: scripts/check-linux-binary.sh <target-triple> <binary>...
set -euo pipefail

# The oldest glibc the x86_64-unknown-linux-gnu archive supports: Ubuntu
# 24.04's, the image that builds it. Older systems take the musl archive.
# Raising this drops systems, so it moves on purpose, never because a
# runner label did.
GLIBC_FLOOR=2.39

READELF=${READELF:-readelf}

if [ $# -lt 2 ]; then
  echo "usage: $0 <target-triple> <binary>..." >&2
  exit 2
fi
target=$1
shift

for bin in "$@"; do
  case $target in
    *-linux-musl)
      if "$READELF" -lW "$bin" | grep -q 'INTERP'; then
        echo "$bin: names a program interpreter, so it is not static" >&2
        exit 1
      fi
      if "$READELF" -dW "$bin" | grep -q '(NEEDED)'; then
        echo "$bin: needs shared libraries:" >&2
        "$READELF" -dW "$bin" | grep '(NEEDED)' >&2
        exit 1
      fi
      echo "$bin: static"
      ;;
    *-linux-gnu)
      needed=$("$READELF" -VW "$bin" | grep -o 'GLIBC_[0-9][0-9.]*' | sed 's/^GLIBC_//' | sort -uV | tail -n 1)
      if [ -z "$needed" ]; then
        echo "$bin: no glibc version requirements found" >&2
        exit 1
      fi
      newest=$(printf '%s\n%s\n' "$needed" "$GLIBC_FLOOR" | sort -V | tail -n 1)
      if [ "$newest" != "$GLIBC_FLOOR" ]; then
        echo "$bin: needs glibc $needed, newer than the $GLIBC_FLOOR floor" >&2
        exit 1
      fi
      echo "$bin: needs glibc $needed (floor $GLIBC_FLOOR)"
      ;;
    *)
      echo "$target: not a Linux target this script checks" >&2
      exit 2
      ;;
  esac

  if [ "$(uname -s)-$(uname -m)" = "Linux-${target%%-*}" ]; then
    "$bin" --version
  else
    echo "$bin: not run; this host is $(uname -s)-$(uname -m)"
  fi
done
