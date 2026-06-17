# Build del cliente `skry` en Windows

El cliente `skry` enlaza FFmpeg (decode) y SDL2 (render). En Windows, FFmpeg se
provee con **vcpkg** (estático) y SDL2 con el crate (`bundled` + `static-link`),
para producir un único `.exe` sin DLLs al lado. Ver
[ADR-0007](decisions/0007-build-windows-vcpkg.md).

## Prerequisitos (una sola vez)

En PowerShell:

```powershell
# Compilador C++ de Microsoft (MSVC). Lo necesitan vcpkg (compila FFmpeg desde
# fuente) y Rust MSVC (linker). Descarga grande (varios GB).
winget install --id Microsoft.VisualStudio.2022.BuildTools --override "--quiet --wait --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"

winget install Rustlang.Rustup   # toolchain Rust (usa rust-toolchain.toml -> 1.83)
winget install LLVM.LLVM         # libclang, requerido por ffmpeg-sys-next (bindgen)
```

Reabrir la terminal. Luego vcpkg + FFmpeg (el primer build de FFmpeg tarda
~30-40 min):

```powershell
git clone https://github.com/microsoft/vcpkg C:\vcpkg
C:\vcpkg\bootstrap-vcpkg.bat
C:\vcpkg\vcpkg install "ffmpeg[avcodec,avformat,swscale,swresample]:x64-windows-static-md"
```

(`avutil` no se lista: es el core de FFmpeg, vcpkg siempre lo construye.)
El triplet `x64-windows-static-md` (libs estáticas, CRT dinámico) es el que
combina con el linkeo MSVC por defecto de Rust.

## Compilar

```powershell
git clone https://github.com/eliasss3990/skry C:\skry   # o donde prefieras
cd C:\skry\client

$env:VCPKG_ROOT = "C:\vcpkg"
$env:VCPKGRS_TRIPLET = "x64-windows-static-md"
$env:LIBCLANG_PATH = "C:\Program Files\LLVM\bin"
# CMake 4.x quitó la compatibilidad con minimums < 3.5; el SDL2 bundled lo pide.
$env:CMAKE_POLICY_VERSION_MINIMUM = "3.5"

cargo build --release -p skry
```

Binario: `client\target\release\skry.exe`.

## Correr

Con el teléfono conectado (USB o Wi-Fi por ADB) y el jar del server en
`/data/local/tmp/`:

```powershell
.\target\release\skry.exe
```

Teclas: **F** alterna pantalla completa, **Q** cierra.

> Nota: la parte de linkeo FFmpeg/vcpkg es la más sensible a la configuración del
> entorno. Si `cargo build` falla resolviendo FFmpeg, suele ser `VCPKG_ROOT` /
> `VCPKGRS_TRIPLET` / `LIBCLANG_PATH` mal seteados, o el triplet del `vcpkg
> install` distinto al de `VCPKGRS_TRIPLET`.
