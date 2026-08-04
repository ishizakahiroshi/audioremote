<#
.SYNOPSIS
  assets/icon.svg から MSIX 用のロゴ PNG 一式を焼く（Chrome ヘッドレス・ImageMagick 不要）。

.DESCRIPTION
  出力先は assets/msix/。生成するのは Windows 11 が実際に読む 4 枚だけ:

    Square44x44Logo.png    アプリ一覧・タスクバー・Alt+Tab
    Square71x71Logo.png    小タイル（DefaultTile）
    Square150x150Logo.png  中タイル（スタートにピン留めしたとき）
    StoreLogo.png (50x50)  Store 掲載とインストーラ

  Square310x310Logo は作らない。指定すると Wide310x150Logo も必須になり
  （MakeAppx error 80080204）、Windows 11 では Live Tile 自体が使われないので
  工数だけ増える。

  スケール修飾（.scale-200 等）も作らない。修飾子の解決には resources.pri が要り、
  PRI 無しのパッケージでは素のファイル名しか読まれないため、置いても死に資産になる。

.EXAMPLE
  pwsh -NoProfile -File packaging/msix/gen-logos.ps1
#>
param(
  [string]$Svg,
  [string]$OutDir
)

$ErrorActionPreference = "Stop"

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$RepoRoot = Split-Path -Parent (Split-Path -Parent $ScriptDir)
if (-not $Svg) { $Svg = Join-Path $RepoRoot "assets\icon.svg" }
if (-not $OutDir) { $OutDir = Join-Path $RepoRoot "assets\msix" }

if (-not (Test-Path -LiteralPath $Svg)) { throw "SVG がありません: $Svg" }

$chrome = @(
  "C:\Program Files\Google\Chrome\Application\chrome.exe",
  "C:\Program Files (x86)\Google\Chrome\Application\chrome.exe",
  "$env:LOCALAPPDATA\Google\Chrome\Application\chrome.exe"
) | Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $chrome) { throw "Chrome が見つかりません（ヘッドレス描画に使います）" }

if (-not (Test-Path -LiteralPath $OutDir)) { New-Item -ItemType Directory -Path $OutDir -Force | Out-Null }

# 名前 → 一辺のピクセル数。ファイル名の数字と実寸を必ず一致させる
# （AppxManifest が宣言する公称サイズとズレると、拡大でにじむ）。
$Logos = [ordered]@{
  'Square44x44Logo.png'   = 44
  'Square71x71Logo.png'   = 71
  'Square150x150Logo.png' = 150
  'StoreLogo.png'         = 50
}

$svgRaw = Get-Content -Raw -LiteralPath $Svg
$tmpRoot = Join-Path $env:TEMP ("msix-logos-" + [System.IO.Path]::GetRandomFileName())
New-Item -ItemType Directory -Path $tmpRoot | Out-Null

try {
  foreach ($name in $Logos.Keys) {
    $n = $Logos[$name]
    $html = @"
<!DOCTYPE html><html><head><meta charset="UTF-8"><style>
  html,body{margin:0;padding:0;background:transparent}
  .frame{width:${n}px;height:${n}px;overflow:hidden}
  .frame svg{width:${n}px;height:${n}px;display:block}
</style></head><body><div class="frame">$svgRaw</div></body></html>
"@
    $tmpHtml = Join-Path $tmpRoot "$n.html"
    Set-Content -LiteralPath $tmpHtml -Value $html -Encoding UTF8
    $uri = ([System.Uri]$tmpHtml).AbsoluteUri
    $out = Join-Path $OutDir $name
    if (Test-Path -LiteralPath $out) { Remove-Item -LiteralPath $out -Force }

    & $chrome --headless=new --disable-gpu --hide-scrollbars --allow-file-access-from-files `
      --default-background-color=00000000 --force-device-scale-factor=1 `
      --window-size="$n,$n" --screenshot="$out" $uri 2>$null | Out-Null

    if (-not (Test-Path -LiteralPath $out)) { throw "描画に失敗しました: $name (${n}x${n})" }
    "OK   {0,-24} {1}x{1}" -f $name, $n
  }
}
finally {
  Remove-Item -Recurse -Force -LiteralPath $tmpRoot -ErrorAction SilentlyContinue
}

""
"出力先: $OutDir"
Get-ChildItem -LiteralPath $OutDir | Select-Object Name, Length | Format-Table -AutoSize | Out-String | Write-Output
