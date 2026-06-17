# Build multi-stage del binario de release de skry (Linux).
#
# Produce un artefacto LIMPIO: la imagen final no contiene el toolchain de Rust,
# ni el código fuente, ni dependencias de build — sólo el binario y las libs de
# runtime. Corre como usuario sin privilegios (no root).
#
# Extraer el binario sin imagen intermedia:
#   docker build -f build/release.Dockerfile --target export --output type=local,dest=dist .
#
# Construir la imagen runnable (headless):
#   docker build -f build/release.Dockerfile -t skry:local .

# ---- Stage 1: builder (pesado, descartable) -------------------------------
FROM rust:1.83-bookworm AS builder

RUN apt-get update && apt-get install -y --no-install-recommends \
        pkg-config clang \
        libavcodec-dev libavformat-dev libavutil-dev libavdevice-dev \
        libswscale-dev libsdl2-dev \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --create-home --no-log-init --uid 10001 builder

# Compilar como usuario no-root (mínimo privilegio, ADR-0006). WORKDIR tras USER
# crea /src con dueño builder, así cargo puede escribir target/.
USER builder
WORKDIR /src
COPY --chown=builder:builder client/ .
# --locked: respeta Cargo.lock, build reproducible. El jar del server se embebe
# en el binario; debe estar presente antes de este build (lo coloca la CI).
RUN cargo build --release --locked

# ---- Stage 2: runtime (mínimo, sin toolchain ni fuentes) ------------------
FROM debian:bookworm-slim AS runtime

# Sólo las libs de runtime (no las -dev). Sin compiladores ni fuentes.
RUN apt-get update && apt-get install -y --no-install-recommends \
        libavcodec59 libavformat59 libavutil57 libavdevice59 \
        libswscale6 libsdl2-2.0-0 \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --create-home --no-log-init --uid 10001 skry

COPY --from=builder /src/target/release/skry /usr/local/bin/skry

# Mínimo privilegio: corre como usuario no-root.
USER skry
WORKDIR /home/skry
ENTRYPOINT ["skry"]

# ---- Stage export: sólo el binario, para extraerlo con --output ------------
FROM scratch AS export
COPY --from=builder /src/target/release/skry /skry
