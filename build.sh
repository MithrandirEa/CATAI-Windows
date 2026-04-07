#!/bin/bash
set -e

echo "Building CATAI.app..."

# Clean
rm -rf CATAI.app CATAI.icns CATAI.iconset

# Create icon from sprite
mkdir -p CATAI.iconset
SRC=cute_orange_cat/rotations/south.png
for size in 16 32 64 128 256 512; do
    sips -z $size $size "$SRC" --out "CATAI.iconset/icon_${size}x${size}.png" >/dev/null 2>&1
done
for size in 16 32 64 128 256; do
    double=$((size * 2))
    cp "CATAI.iconset/icon_${double}x${double}.png" "CATAI.iconset/icon_${size}x${size}@2x.png"
done
sips -z 1024 1024 "$SRC" --out "CATAI.iconset/icon_512x512@2x.png" >/dev/null 2>&1
iconutil -c icns CATAI.iconset -o CATAI.icns
rm -rf CATAI.iconset

# Create .app structure
mkdir -p CATAI.app/Contents/MacOS CATAI.app/Contents/Resources
cp CATAI.icns CATAI.app/Contents/Resources/
cp -R cute_orange_cat CATAI.app/Contents/Resources/

# Info.plist
cat > CATAI.app/Contents/Info.plist << 'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>CFBundleExecutable</key>
	<string>CATAI</string>
	<key>CFBundleIdentifier</key>
	<string>com.wilpe.catai</string>
	<key>CFBundleName</key>
	<string>CATAI</string>
	<key>CFBundleDisplayName</key>
	<string>CATAI</string>
	<key>CFBundleVersion</key>
	<string>1.0</string>
	<key>CFBundleShortVersionString</key>
	<string>1.0</string>
	<key>CFBundlePackageType</key>
	<string>APPL</string>
	<key>CFBundleIconFile</key>
	<string>CATAI</string>
	<key>LSMinimumSystemVersion</key>
	<string>14.0</string>
	<key>LSUIElement</key>
	<true/>
	<key>NSHighResolutionCapable</key>
	<true/>
</dict>
</plist>
PLIST

# Compile
swiftc -O -o CATAI.app/Contents/MacOS/CATAI cat.swift -framework AppKit -framework Foundation

echo "✓ CATAI.app built successfully!"
echo "Run: open CATAI.app"
