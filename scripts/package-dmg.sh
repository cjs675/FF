#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TARGET_DIR="${REPO_ROOT}/target"
RELEASE_DIR="${TARGET_DIR}/release"
BIN_PATH="${RELEASE_DIR}/ff"
BUNDLE_DIR="${RELEASE_DIR}/bundle"
APP_DIR="${BUNDLE_DIR}/FF.app"
DMG_STAGING="${BUNDLE_DIR}/dmg_staging"
OUTPUT_DMG="${TARGET_DIR}/release/FF.dmg"

echo "==> Building release binary..."
cargo build --release --workspace

echo "==> Preparing macOS App Bundle (${APP_DIR})..."
rm -rf "${BUNDLE_DIR}"
mkdir -p "${APP_DIR}/Contents/MacOS"
mkdir -p "${APP_DIR}/Contents/Resources"

# Copy binary and strip debug symbols
cp "${BIN_PATH}" "${APP_DIR}/Contents/MacOS/ff"
chmod +x "${APP_DIR}/Contents/MacOS/ff"
strip "${APP_DIR}/Contents/MacOS/ff" || true

# Copy AppIcon.icns
cp "${REPO_ROOT}/crates/app/assets/icons/AppIcon.icns" "${APP_DIR}/Contents/Resources/AppIcon.icns"

# Generate Info.plist
cat << 'EOF' > "${APP_DIR}/Contents/Info.plist"
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleExecutable</key>
    <string>ff</string>
    <key>CFBundleIdentifier</key>
    <string>dev.ff.filebrowser</string>
    <key>CFBundleName</key>
    <string>FF</string>
    <key>CFBundleDisplayName</key>
    <string>FF</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>0.1.0</string>
    <key>CFBundleVersion</key>
    <string>1</string>
    <key>CFBundleIconFile</key>
    <string>AppIcon</string>
    <key>LSMinimumSystemVersion</key>
    <string>11.0</string>
    <key>NSHighResolutionCapable</key>
    <true/>
    <key>NSSupportsAutomaticGraphicsSwitching</key>
    <true/>
</dict>
</plist>
EOF

echo "==> Preparing DMG staging area..."
mkdir -p "${DMG_STAGING}"
cp -R "${APP_DIR}" "${DMG_STAGING}/"
ln -s /Applications "${DMG_STAGING}/Applications"

echo "==> Creating compressed disk image (${OUTPUT_DMG})..."
rm -f "${OUTPUT_DMG}"
hdiutil create \
    -volname "FF" \
    -srcfolder "${DMG_STAGING}" \
    -ov \
    -format UDZO \
    "${OUTPUT_DMG}"

# Clean up staging files
rm -rf "${DMG_STAGING}"

echo "==> DMG successfully created:"
ls -lh "${OUTPUT_DMG}"
