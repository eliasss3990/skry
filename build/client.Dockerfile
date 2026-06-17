# Imagen de build del cliente Rust de skry.
#
# Toolchain reproducible e idéntico a la CI (ver ADR-0004). Incluye las libs
# nativas que el cliente necesita para decode (FFmpeg) y render (SDL2), de modo
# que el mismo contenedor sirve para los crates puros (proto, adb) y para el de
# video. El runtime real corre nativo en el host, no acá.
#
# Build:  docker build -t skry-build-client:local -f build/client.Dockerfile .
# Uso:    via scripts/dev (mapea el usuario del host y cachea cargo).
FROM rust:1.83-bookworm

# Dependencias nativas: FFmpeg (libav*), SDL2, y pkg-config/clang para los
# bindings -sys. Versiones pineadas por la distro (bookworm), no 'latest'.
RUN apt-get update && apt-get install -y --no-install-recommends \
        pkg-config \
        clang \
        libavcodec-dev \
        libavformat-dev \
        libavutil-dev \
        libavdevice-dev \
        libswscale-dev \
        libsdl2-dev \
    && rm -rf /var/lib/apt/lists/*

# Componentes de lint/formato dentro de la imagen (paridad con rust-toolchain.toml).
RUN rustup component add clippy rustfmt

WORKDIR /work
