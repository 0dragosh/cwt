#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
verification_dir="$repo_root/verification"

required_rust_toolchain() {
  case "$(uname -s):$(uname -m)" in
    Darwin:arm64|Darwin:aarch64)
      echo "1.95.0-aarch64-apple-darwin"
      ;;
    Darwin:x86_64)
      echo "1.95.0-x86_64-apple-darwin"
      ;;
    Linux:x86_64)
      echo "1.95.0-x86_64-unknown-linux-gnu"
      ;;
    *)
      echo "error: unsupported host for pinned Verus release: $(uname -s) $(uname -m)" >&2
      exit 1
      ;;
  esac
}

if ! command -v verus >/dev/null 2>&1; then
  echo "error: verus is not on PATH" >&2
  echo "hint: run via nix develop .#verus -c ./scripts/verify-verus.sh" >&2
  exit 127
fi

if ! command -v rustup >/dev/null 2>&1; then
  toolchain="$(required_rust_toolchain)"
  echo "error: rustup is not on PATH" >&2
  echo "Verus requires Rust toolchain $toolchain" >&2
  echo "install it with: rustup install $toolchain" >&2
  exit 127
fi

toolchain="$(required_rust_toolchain)"
if ! rustup run "$toolchain" rustc --version >/dev/null 2>&1; then
  echo "error: required Rust toolchain is not installed: $toolchain" >&2
  echo "install it with: rustup install $toolchain" >&2
  exit 1
fi

found=0
while IFS= read -r file; do
  found=1
  echo "verus ${file#$repo_root/}"
  verus "$file"
done < <(find "$verification_dir" -type f -name '*.rs' | LC_ALL=C sort)

if [[ "$found" -eq 0 ]]; then
  echo "error: no Verus files found under ${verification_dir#$repo_root/}" >&2
  exit 1
fi
