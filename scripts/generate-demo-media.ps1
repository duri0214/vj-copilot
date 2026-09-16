param(
    [string]$OutputDir = (Join-Path $PSScriptRoot "..\demo-media")
)

$ErrorActionPreference = "Stop"

$ffmpeg = Get-Command ffmpeg -ErrorAction SilentlyContinue
if ($null -eq $ffmpeg) {
    throw "ffmpeg が PATH にありません。README の必要なものを確認してください。"
}

$resolvedOutputDir = [System.IO.Path]::GetFullPath($OutputDir)
New-Item -ItemType Directory -Path $resolvedOutputDir -Force | Out-Null

$clips = @(
    @{ file = "low_dark_01.mp4"; energy = 0.2; brightness = 0.2; box = "0x3050d0"; speed = 20 },
    @{ file = "low_dark_02.mp4"; energy = 0.2; brightness = 0.2; box = "0x5030b0"; speed = 24 },
    @{ file = "low_bright_01.mp4"; energy = 0.2; brightness = 0.8; box = "0x30d0d0"; speed = 20 },
    @{ file = "low_bright_02.mp4"; energy = 0.2; brightness = 0.8; box = "0x50d080"; speed = 24 },
    @{ file = "high_dark_01.mp4"; energy = 0.8; brightness = 0.2; box = "0xd05030"; speed = 120 },
    @{ file = "high_dark_02.mp4"; energy = 0.8; brightness = 0.2; box = "0xd03080"; speed = 140 },
    @{ file = "high_bright_01.mp4"; energy = 0.8; brightness = 0.8; box = "0xf0d030"; speed = 120 },
    @{ file = "high_bright_02.mp4"; energy = 0.8; brightness = 0.8; box = "0x90f040"; speed = 140 }
)

foreach ($clip in $clips) {
    $outputPath = Join-Path $resolvedOutputDir $clip.file
    $brightnessOffset = if ($clip.brightness -lt 0.5) { "-0.35" } else { "0.25" }
    $filter = "testsrc2=size=320x180:rate=15:duration=3,eq=brightness={0},drawbox=x='mod(t*{1}\,280)':y=70:w=40:h=40:color={2}:t=fill" -f $brightnessOffset, $clip.speed, $clip.box

    & $ffmpeg.Source -hide_banner -y -f lavfi -i $filter -t 3 -an -c:v libx264 -pix_fmt yuv420p -r 15 $outputPath
    if ($LASTEXITCODE -ne 0) {
        throw "ダミー動画の生成に失敗しました: $($clip.file)"
    }
}

$metadata = $clips | ForEach-Object {
    [ordered]@{
        file = $_.file
        energy = $_.energy
        brightness = $_.brightness
    }
}
$json = $metadata | ConvertTo-Json
$utf8WithoutBom = [System.Text.UTF8Encoding]::new($false)
[System.IO.File]::WriteAllText((Join-Path $resolvedOutputDir "clips.json"), $json, $utf8WithoutBom)

Write-Host "8 本のダミー MP4 と clips.json を生成しました: $resolvedOutputDir"
