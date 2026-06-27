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
ENV CMDLINE_TOOLS_SHA256=2d2d50857e4eb553af5a6dc3ad507a17adf43d115264b1afc116f95c92e5e258
ENV ANDROID_PLATFORM=android-36
ENV ANDROID_BUILD_TOOLS=36.0.0
# Plataforma/herramientas para la app nativa Kotlin (compileSdk 35) y Gradle
# pineado para su build (ADR 0008). Aditivo: el spike (API 36) queda intacto.
ENV ANDROID_APP_PLATFORM=android-35
ENV ANDROID_APP_BUILD_TOOLS=35.0.0
ENV GRADLE_VERSION=8.11.1

RUN apt-get update && apt-get install -y --no-install-recommends \
        curl unzip \
    && rm -rf /var/lib/apt/lists/*

# Descargar cmdline-tools (con verificación de checksum) e instalar plataforma +
# build-tools de API 36.
RUN mkdir -p "$ANDROID_SDK_ROOT/cmdline-tools" \
    && curl -fsSL "https://dl.google.com/android/repository/commandlinetools-linux-${CMDLINE_TOOLS_VERSION}_latest.zip" -o /tmp/cmdline-tools.zip \
    && echo "${CMDLINE_TOOLS_SHA256}  /tmp/cmdline-tools.zip" | sha256sum -c - \
    && unzip -q /tmp/cmdline-tools.zip -d "$ANDROID_SDK_ROOT/cmdline-tools" \
    && mv "$ANDROID_SDK_ROOT/cmdline-tools/cmdline-tools" "$ANDROID_SDK_ROOT/cmdline-tools/latest" \
    && rm /tmp/cmdline-tools.zip

ENV PATH="$ANDROID_SDK_ROOT/cmdline-tools/latest/bin:$ANDROID_SDK_ROOT/platform-tools:$PATH"

RUN yes | sdkmanager --licenses >/dev/null \
    && sdkmanager --install \
        "platform-tools" \
        "platforms;${ANDROID_PLATFORM}" \
        "build-tools;${ANDROID_BUILD_TOOLS}" \
        "platforms;${ANDROID_APP_PLATFORM}" \
        "build-tools;${ANDROID_APP_BUILD_TOOLS}" \
    && chmod -R a+rX "$ANDROID_SDK_ROOT"

# Gradle pineado (verificado contra el sha256 publicado por Gradle). Para el
# build de la app nativa; el jar del spike usa javac+d8 directo, sin Gradle.
ENV GRADLE_HOME=/opt/gradle
RUN curl -fsSL "https://services.gradle.org/distributions/gradle-${GRADLE_VERSION}-bin.zip" -o /tmp/gradle.zip \
    && curl -fsSL "https://services.gradle.org/distributions/gradle-${GRADLE_VERSION}-bin.zip.sha256" -o /tmp/gradle.zip.sha256 \
    && echo "$(cat /tmp/gradle.zip.sha256)  /tmp/gradle.zip" | sha256sum -c - \
    && unzip -q /tmp/gradle.zip -d /opt \
    && mv "/opt/gradle-${GRADLE_VERSION}" "$GRADLE_HOME" \
    && rm /tmp/gradle.zip /tmp/gradle.zip.sha256
ENV PATH="$GRADLE_HOME/bin:$PATH"

# Mínimo privilegio: no correr como root.
RUN useradd --create-home --no-log-init --uid 10001 builder
USER builder
WORKDIR /work
