# Imagen de build del server Android de skry (toolchain del SDK).
#
# Compila y dexea los .jar que corren via app_process en el teléfono. Toolchain
# reproducible (cmdline-tools y paquetes pineados), corre como usuario no-root
# (ADR-0006). El runtime real es el teléfono, no esta imagen.
#
# Build: docker build -t skry-build-android:local -f build/android.Dockerfile .
FROM eclipse-temurin:21-jdk-jammy

# Versión pineada de cmdline-tools (no 'latest' mutable).
ENV ANDROID_SDK_ROOT=/opt/android-sdk
ENV CMDLINE_TOOLS_VERSION=11076708
ENV ANDROID_PLATFORM=android-36
ENV ANDROID_BUILD_TOOLS=36.0.0

RUN apt-get update && apt-get install -y --no-install-recommends \
        curl unzip \
    && rm -rf /var/lib/apt/lists/*

# Descargar cmdline-tools e instalar plataforma + build-tools de API 36.
RUN mkdir -p "$ANDROID_SDK_ROOT/cmdline-tools" \
    && curl -fsSL "https://dl.google.com/android/repository/commandlinetools-linux-${CMDLINE_TOOLS_VERSION}_latest.zip" -o /tmp/cmdline-tools.zip \
    && unzip -q /tmp/cmdline-tools.zip -d "$ANDROID_SDK_ROOT/cmdline-tools" \
    && mv "$ANDROID_SDK_ROOT/cmdline-tools/cmdline-tools" "$ANDROID_SDK_ROOT/cmdline-tools/latest" \
    && rm /tmp/cmdline-tools.zip

ENV PATH="$ANDROID_SDK_ROOT/cmdline-tools/latest/bin:$ANDROID_SDK_ROOT/platform-tools:$PATH"

RUN yes | sdkmanager --licenses >/dev/null \
    && sdkmanager --install \
        "platforms;${ANDROID_PLATFORM}" \
        "build-tools;${ANDROID_BUILD_TOOLS}" \
    && chmod -R a+rX "$ANDROID_SDK_ROOT"

# Mínimo privilegio: no correr como root.
RUN useradd --create-home --no-log-init --uid 10001 builder
USER builder
WORKDIR /work
