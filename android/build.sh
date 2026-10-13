#!/usr/bin/env bash
# Build the Desktop Workspace phone app (APK) with the Android SDK's own tools:
# no Gradle and no downloads.
#
#   ./build.sh            -> build/desktop-workspace.apk
#   ./build.sh install    -> also install it on the phone connected with adb
#
# ANDROID_HOME (or ANDROID_SDK_ROOT) points at an SDK with build-tools and a
# platform; JAVA_HOME at a JDK 17. The signing key is created once in
# ~/.android/desktop-workspace.keystore: keep it, because a phone only accepts
# an update signed with the same key.
set -euo pipefail
cd "$(dirname "$0")"

SDK="${ANDROID_HOME:-${ANDROID_SDK_ROOT:-$HOME/.local/opt/neural-tycoon-toolchain/android-sdk}}"
BT="$(ls -d "$SDK"/build-tools/* | sort -V | tail -1)"
PLATFORM="$(ls -d "$SDK"/platforms/android-* | sort -V | tail -1)/android.jar"
JDK="${JAVA_HOME:-$HOME/.local/opt/neural-tycoon-toolchain/jdk-17}"
KEYSTORE="${DW_KEYSTORE:-$HOME/.android/desktop-workspace.keystore}"
VERSION_CODE=1
VERSION_NAME=0.1.0

rm -rf build
mkdir -p build/gen build/classes build/dex

"$BT/aapt2" compile --dir res -o build/res.zip
"$BT/aapt2" link -I "$PLATFORM" --manifest AndroidManifest.xml -o build/base.apk \
    --java build/gen --min-sdk-version 26 --target-sdk-version 35 \
    --version-code "$VERSION_CODE" --version-name "$VERSION_NAME" build/res.zip

"$JDK/bin/javac" -nowarn -Xlint:-options --release 11 -encoding UTF-8 -cp "$PLATFORM" \
    -d build/classes $(find src build/gen -name '*.java')
"$BT/d8" --release --min-api 26 --lib "$PLATFORM" --output build/dex \
    $(find build/classes -name '*.class')

cp build/base.apk build/unsigned.apk
(cd build/dex && "$JDK/bin/jar" uf ../unsigned.apk classes.dex)
"$BT/zipalign" -f -p 4 build/unsigned.apk build/aligned.apk

if [[ ! -f "$KEYSTORE" ]]; then
    mkdir -p "$(dirname "$KEYSTORE")"
    "$JDK/bin/keytool" -genkeypair -keystore "$KEYSTORE" -storepass desktop-workspace \
        -alias app -keyalg RSA -keysize 3072 -validity 10000 \
        -dname "CN=Desktop Workspace" >/dev/null
    echo "Created signing key $KEYSTORE (keep it: updates need the same key)"
fi
"$BT/apksigner" sign --ks "$KEYSTORE" --ks-pass pass:desktop-workspace \
    --out build/desktop-workspace.apk build/aligned.apk
"$BT/apksigner" verify build/desktop-workspace.apk
echo "Built $(pwd)/build/desktop-workspace.apk"

if [[ "${1:-}" == "install" ]]; then
    "$SDK/platform-tools/adb" install -r build/desktop-workspace.apk
fi
