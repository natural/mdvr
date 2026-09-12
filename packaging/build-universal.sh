#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
out="$root/target/release/mdvr"

if [ -x /opt/homebrew/opt/rustup/bin/rustup ]; then
    PATH=/opt/homebrew/opt/rustup/bin:$PATH
    RUSTUP_TOOLCHAIN=1.98.1
    export PATH RUSTUP_TOOLCHAIN
elif command -v rustup >/dev/null 2>&1; then
    RUSTUP_TOOLCHAIN=1.98.1
    export RUSTUP_TOOLCHAIN
fi

for command in cargo rustc; do
    command -v "$command" >/dev/null 2>&1 || {
        echo "packaging: required command missing: $command" >&2
        exit 1
    }
done
command -v lipo >/dev/null 2>&1 || {
    echo "packaging: lipo missing" >&2
    exit 1
}
for target in aarch64-apple-darwin x86_64-apple-darwin; do
    target_libdir=$(rustc --print target-libdir --target "$target")
    [ -d "$target_libdir" ] || {
        echo "packaging: Rust target unavailable: $target" >&2
        exit 1
    }
    cargo build --release --locked --target "$target"
done
mkdir -p "$(dirname "$out")"
lipo -create \
    "$root/target/aarch64-apple-darwin/release/mdvr" \
    "$root/target/x86_64-apple-darwin/release/mdvr" \
    -output "$out"
case "$(lipo -archs "$out")" in
"x86_64 arm64" | "arm64 x86_64") ;;
*)
    echo "packaging: universal output has wrong architectures" >&2
    exit 1
    ;;
esac
echo "packaging: created universal $out"
