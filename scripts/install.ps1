# Instalador de skry para Windows. Descarga el binario del último release, lo
# deja en %LOCALAPPDATA%\skry y agrega esa carpeta al PATH del usuario para
# invocar `skry` desde cualquier carpeta.
#
#   irm https://raw.githubusercontent.com/eliasss3990/skry/main/scripts/install.ps1 | iex
#
# Variables opcionales:
#   $env:SKRY_VERSION = 'v1.2.3'   instala una versión puntual (default: latest)

$ErrorActionPreference = 'Stop'

$repo = 'eliasss3990/skry'
$asset = 'skry-x86_64-pc-windows-msvc.exe'
$binName = 'skry.exe'

$version = if ($env:SKRY_VERSION) { $env:SKRY_VERSION } else { 'latest' }
if ($version -eq 'latest') {
    $url = "https://github.com/$repo/releases/latest/download/$asset"
} else {
    $url = "https://github.com/$repo/releases/download/$version/$asset"
}

$installDir = Join-Path $env:LOCALAPPDATA 'skry'
$dest = Join-Path $installDir $binName

Write-Host "[install] descargando $asset ($version)..."
New-Item -ItemType Directory -Force -Path $installDir | Out-Null
try {
    Invoke-WebRequest -Uri $url -OutFile $dest -UseBasicParsing
} catch {
    throw "[install] no se pudo descargar $url (¿existe ya un release para Windows?)"
}

# Inyectar el directorio en el PATH del usuario (persistente, sin admin).
$userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
if ($null -eq $userPath) { $userPath = '' }
$paths = $userPath.Split(';') | Where-Object { $_ -ne '' }
if ($paths -notcontains $installDir) {
    $newPath = (@($paths) + $installDir) -join ';'
    [Environment]::SetEnvironmentVariable('Path', $newPath, 'User')
    Write-Host "[install] agregado $installDir al PATH del usuario."
    Write-Host "[install] abrí una terminal nueva para que tome el PATH."
}

Write-Host "[install] instalado en $dest"
Write-Host "[install] listo. Probá: skry --help"
