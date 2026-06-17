#!/usr/bin/env bash
# Compila y dexea el Spike 1 en un .jar que corre via app_process en el teléfono.
# Pensado para correr DENTRO de la imagen skry-build-android (ver build/android.Dockerfile),
# con el repo montado en /work. Produce dist/skry-spike.jar.
set -euo pipefail

SDK="${ANDROID_SDK_ROOT:-/opt/android-sdk}"
ANDROID_JAR="$SDK/platforms/android-36/android.jar"
D8="$SDK/build-tools/36.0.0/d8"

SRC_DIR="server/spike/src"
WORK="server/spike/build"
DIST="dist"
JAR="$DIST/skry-spike.jar"

rm -rf "$WORK"
mkdir -p "$WORK/classes" "$DIST"

echo "[spike] compilando contra android.jar (API 36)..."
find "$SRC_DIR" -name '*.java' > "$WORK/sources.txt"
javac -source 17 -target 17 -cp "$ANDROID_JAR" -d "$WORK/classes" @"$WORK/sources.txt"

echo "[spike] dexeando con d8..."
find "$WORK/classes" -name '*.class' > "$WORK/classes.txt"
"$D8" --output "$WORK" --lib "$ANDROID_JAR" @"$WORK/classes.txt"

echo "[spike] empaquetando $JAR..."
( cd "$WORK" && jar cf "../../../$JAR" classes.dex )

echo "[spike] listo: $JAR"
ls -l "$JAR"
