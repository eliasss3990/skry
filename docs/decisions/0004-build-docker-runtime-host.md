# ADR 0004: Build dockerizado, runtime nativo en el host

- Estado: Aceptada
- Fecha: 2026-06-16

## Contexto

El cliente Rust depende de libs nativas pesadas (FFmpeg, SDL2) y debe
cross-compilar a Windows. El server Android necesita el SDK de Android + Gradle.
Instalar todo eso suelto en cada máquina de desarrollo es frágil y no
reproducible, y diverge de lo que corre en CI.

Por otro lado, el **runtime** del cliente necesita acceso a hardware del host:
USB (para ADB con el teléfono), GPU (decode por hardware) y un servidor de
ventanas (render SDL2). Eso no vive cómodo dentro de un contenedor.

## Decisión

- **Build**: dentro de Docker. Una imagen para el toolchain Rust (con headers de
  FFmpeg/SDL2 y soporte de cross-compile a Windows), otra para Android (SDK
  cmdline-tools + Gradle + JDK). Es la **misma imagen que usa la CI**, lo que
  garantiza paridad build local ↔ CI. Wrapper de invocación en `scripts/`.
- **Runtime**: **nativo en el host**. Correr `skry` para espejar el teléfono usa
  el ADB, la GPU y la ventana del sistema operativo del usuario directamente.

Esto es coherente con la regla docker-first y su excepción de bootstrapping:
Docker para todo lo que sea toolchain reproducible; nativo sólo donde el acceso
directo a hardware lo exige, documentado acá como excepción justificada.

## Consecuencias

- **Positivas**: builds reproducibles e idénticos a CI; cero contaminación del
  host con toolchains; onboarding de otra máquina = tener Docker.
- **Negativas**: el ciclo de compilación paga el costo de Docker (mitigable con
  caché de capas y de `target/` montado). La validación final del stream es
  siempre en el host del usuario, no en CI.
