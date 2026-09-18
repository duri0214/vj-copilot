[CmdletBinding()]
param()

$ErrorActionPreference = "Stop"

$projectRoot = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot ".."))
$asioSdkRoot = Join-Path $projectRoot "tools\asio-sdk"
$llvmBin = Join-Path $projectRoot "tools\llvm\bin"

$requiredFiles = @(
    [PSCustomObject]@{
        Path = Join-Path $asioSdkRoot "common\asio.h"
        Description = "ASIO SDK header"
    }
    [PSCustomObject]@{
        Path = Join-Path $asioSdkRoot "common\asiosys.h"
        Description = "ASIO system header"
    }
    [PSCustomObject]@{
        Path = Join-Path $asioSdkRoot "host\asiodrivers.h"
        Description = "ASIO driver header"
    }
    [PSCustomObject]@{
        Path = Join-Path $asioSdkRoot "host\pc\asiolist.cpp"
        Description = "Windows ASIO host source"
    }
    [PSCustomObject]@{
        Path = Join-Path $llvmBin "libclang.dll"
        Description = "LLVM/Clang runtime"
    }
)

$missingFiles = @($requiredFiles | Where-Object { -not (Test-Path -LiteralPath $_.Path -PathType Leaf) })
if ($missingFiles.Count -gt 0) {
    Write-Host "ASIO ビルドの準備が不足しています。" -ForegroundColor Red
    Write-Host "次のファイルをプロジェクト内へ配置してください:" -ForegroundColor Yellow
    foreach ($missingFile in $missingFiles) {
        Write-Host ("  - {0}: {1}" -f $missingFile.Description, $missingFile.Path)
    }
    Write-Host ""
    Write-Host "配置方法は README.md の『ASIO 入力を追加したビルド』を確認してください。" -ForegroundColor Yellow
    Write-Host "ASIO SDK は common と host を含む展開済みSDKのルートを tools\asio-sdk に置きます。"
    Write-Host "LLVM は bin\libclang.dll を tools\llvm\bin に置きます。"
    exit 1
}

$cargo = Get-Command cargo -CommandType Application -ErrorAction SilentlyContinue
if ($null -eq $cargo) {
    Write-Error "cargo が見つかりません。Rust の stable MSVC toolchain をインストールしてください。"
    exit 1
}

if ($asioSdkRoot.Contains("'") -or $llvmBin.Contains("'")) {
    Write-Error "プロジェクトのパスにシングルクォートが含まれているため、ASIOビルド設定を作成できません。別の場所へ移動してください。"
    exit 1
}

$cargoConfigPath = Join-Path $projectRoot ("target\asio-build-config-{0}.toml" -f $PID)
$cargoConfig = @"
[env]
CPAL_ASIO_DIR = { value = '$asioSdkRoot', force = true }
LIBCLANG_PATH = { value = '$llvmBin', force = true }
"@

New-Item -ItemType Directory -Force -Path (Split-Path -Parent $cargoConfigPath) | Out-Null
[System.IO.File]::WriteAllText(
    $cargoConfigPath,
    $cargoConfig,
    (New-Object System.Text.UTF8Encoding($false))
)

Push-Location $projectRoot
try {
    Write-Host "ASIO feature を有効にして release build を開始します..." -ForegroundColor Cyan
    & $cargo.Source --config $cargoConfigPath build --release --features asio
    if ($LASTEXITCODE -ne 0) {
        Write-Error "ASIOビルドに失敗しました。Visual Studio C++ Build Tools と配置したSDK/LLVMを確認してください。"
        exit $LASTEXITCODE
    }
}
finally {
    Pop-Location
    Remove-Item -LiteralPath $cargoConfigPath -Force -ErrorAction SilentlyContinue
}

$outputPath = Join-Path $projectRoot "target\release\vj-copilot.exe"
Write-Host ("ASIO対応EXEを作成しました: {0}" -f $outputPath) -ForegroundColor Green
