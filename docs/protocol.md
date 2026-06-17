# Protocolo skry (v1)

Contrato de comunicación entre el cliente (Rust, PC) y el server (Kotlin,
Android). Diseñado para ser **portable entre lenguajes**: codificación binaria
explícita en *big-endian* (orden de red), sin depender del formato interno de
ninguna librería de serialización.

## Transporte

Sobre el túnel ADB. El server escucha en un *localabstract socket* llamado
`skry`. El cliente hace `adb forward tcp:<puerto-local> localabstract:skry` y
abre **dos conexiones** al puerto local, en orden:

1. **Primera conexión = canal de video** (server → cliente, unidireccional).
2. **Segunda conexión = canal de control** (bidireccional).

Un solo `forward`, dos sockets. El server distingue los canales por el orden de
aceptación (igual enfoque que scrcpy).

## Tipos primitivos

| Tipo | Tamaño | Notas |
|------|--------|-------|
| `u8` `u16` `u32` `u64` `i32` | 1/2/4/8/4 bytes | big-endian, sin signo salvo `i32` |
| `bool` | 1 byte | `0x00` = false, `0x01` = true |
| `string` | `u16` longitud + N bytes | UTF-8, sin terminador nulo |
| `enum` | `u8` | discriminante; valores desconocidos = error de protocolo |

## Handshake (canal de video, una vez al conectar)

El server, apenas acepta el canal de video, envía:

```
magic       : 4 bytes  = "SKRY" (0x53 0x4B 0x52 0x59)
version     : u16       = 1
codec       : enum Codec
width       : u16       (px, orientación inicial)
height      : u16       (px)
device_name : string    (ej. "SM-S928B")
```

El cliente valida `magic` y `version`. Si `version` no coincide con la suya,
aborta con mensaje claro (incompatibilidad cliente/server). Los parámetros
deseados (marcha, códec preferido, bitrate, hw-decode) se pasan al server como
**argumentos de invocación** de `app_process` al desplegarlo; el handshake
confirma los efectivos que el dispositivo pudo honrar.

### enum Codec (`u8`)

| Valor | Códec |
|-------|-------|
| 0 | H264 |
| 1 | H265 (HEVC) |

## Canal de video (server → cliente, tras el handshake)

Secuencia de *paquetes de frame*. Cada paquete:

```
pts   : u64   microsegundos, reloj del server (monotónico)
flags : u8    bitfield (ver abajo)
len   : u32   longitud del payload en bytes
payload : len bytes   (unidades NAL del códec)
```

`len` está acotado a `MAX_FRAME_BYTES` (16 MiB) para frenar lecturas absurdas
ante corrupción; un `len` mayor es error de protocolo.

### flags (bitfield `u8`)

| Bit | Nombre | Significado |
|-----|--------|-------------|
| 0 (`0x01`) | `KEYFRAME` | el frame es un keyframe (IDR) |
| 1 (`0x02`) | `CONFIG` | payload de configuración (SPS/PPS/VPS), no es frame visible |

## Canal de control (bidireccional)

Mensajes con un *tag* `u8` inicial que identifica el tipo.

### Cliente → Server

| Tag | Mensaje | Cuerpo |
|-----|---------|--------|
| `0x01` | `SetGear` | `gear: enum Gear` |
| `0x02` | `SetBitrate` | `bitrate: u32` (bits/s) |
| `0x03` | `Ping` | `seq: u32` |
| `0x04` | `Stop` | (sin cuerpo) |

### Server → Cliente

| Tag | Mensaje | Cuerpo |
|-----|---------|--------|
| `0x81` | `Pong` | `seq: u32` |
| `0x82` | `Telemetry` | `encoded_frames: u64`, `dropped_frames: u64`, `bitrate: u32` |
| `0x83` | `GearChanged` | `gear: enum Gear` |
| `0x84` | `Error` | `code: u16`, `message: string` |

Los tags del server tienen el bit alto (`0x80`) seteado: separa visualmente los
dos sentidos y facilita el debugging de capturas.

### enum Gear (`u8`)

| Valor | Marcha | FPS objetivo |
|-------|--------|--------------|
| 0 | `Low` | 60 |
| 1 | `Mid` | 120 |
| 2 | `High` | 144 |

## Versionado

El campo `version` del handshake gobierna la compatibilidad. Cambios que rompen
el wire incrementan `version`; cliente y server sólo operan si coinciden. Esto
permite evolucionar el protocolo sin fallas silenciosas.

## Errores de protocolo

Cualquier desvío (magic inválido, enum desconocido, `len` fuera de rango,
EOF en medio de un mensaje) se trata como error de protocolo: se cierra la
sesión con gracia y se informa al usuario. Nunca se interpreta basura como dato
válido.
