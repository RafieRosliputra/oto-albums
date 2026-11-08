#!/bin/sh
# One-off scaffold of crate manifests (kept for reference).
set -e
cd "$(dirname "$0")/../crates"
mk() { # name kind deps...
  name=$1; kind=$2; shift 2
  f=$name/Cargo.toml
  {
    echo "[package]"
    echo "name = \"$name\""
    echo "version.workspace = true"
    echo "edition.workspace = true"
    echo "license.workspace = true"
    echo "rust-version.workspace = true"
    echo
    if [ "$kind" = bin ]; then echo "[[bin]]"; echo "name = \"$name\""; echo "path = \"src/main.rs\""; echo; fi
    echo "[dependencies]"
    for d in "$@"; do echo "$d.workspace = true"; done
  } > "$f"
}
mk oto-core lib serde serde_json toml uuid time thiserror tracing
mk oto-media lib oto-core serde serde_json uuid time thiserror tracing rayon crossbeam-channel blake3 image kamadak-exif psd
mk oto-store lib oto-core serde serde_json uuid time thiserror tracing rusqlite
mk oto-render lib oto-core image ab_glyph tracing
mk oto-print lib oto-core oto-media oto-render serde serde_json toml uuid time thiserror tracing image pdf-writer subsetter ttf-parser miniz_oxide sha2 zip crossbeam-channel
mk oto-proof lib oto-core oto-print oto-render serde serde_json uuid time thiserror image
mk oto-platform lib rfd thiserror
mk oto-ui lib oto-core oto-media oto-store oto-render oto-print oto-proof oto-platform egui eframe serde serde_json uuid time tracing crossbeam-channel image directories
mk oto-app bin oto-ui eframe tracing tracing-subscriber directories anyhow
mk oto-cli bin oto-core oto-media oto-store oto-render oto-print oto-proof anyhow serde_json toml
