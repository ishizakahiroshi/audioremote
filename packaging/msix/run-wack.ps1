<#
.SYNOPSIS
  Windows App Certification Kit (WACK) で sideload 済みの MSIX を検証する。

.DESCRIPTION
  appcert.exe は管理者権限が必須。**管理者 PowerShell から**実行すること。
  検証中はアプリが自動で起動・終了する（WACK の仕様）。数分かかる。

  audioremote は起動するとトレイに常駐して待受ポートを開くので、WACK が
  起動したインスタンスが残ることがある。終わったらトレイの「終了」で閉じる。

  判定の読み方:
    OPTIONAL=FALSE の FAIL … 直すまで提出しない
    OPTIONAL=TRUE  の FAIL … 許容してよい。Rust 標準ライブラリがプロセス起動 API
                              （CreateProcessW / ShellExecuteW）を参照するため出る。
                              同じ FAIL を出したまま 1 番手は認定に一発合格している。

.EXAMPLE
  # 管理者 PowerShell で
  pwsh -NoProfile -File packaging\msix\run-wack.ps1
#>
$ErrorActionPreference = "Stop"

$appcert = 'C:\Program Files (x86)\Windows Kits\10\App Certification Kit\appcert.exe'
$report = Join-Path $env:TEMP 'wack-audioremote.xml'

Write-Host "== Windows App Certification Kit ==" -ForegroundColor Cyan

if (-not (Test-Path $appcert)) {
  Write-Host "appcert.exe がありません: $appcert" -ForegroundColor Red
  Write-Host "Windows SDK の App Certification Kit を入れてください。" -ForegroundColor Red
  Read-Host "Enter で閉じる"
  exit 1
}

$isAdmin = ([Security.Principal.WindowsPrincipal] [Security.Principal.WindowsIdentity]::GetCurrent()
).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $isAdmin) {
  Write-Host "管理者権限で実行してください（appcert.exe の要件）。" -ForegroundColor Red
  Read-Host "Enter で閉じる"
  exit 1
}

$pkg = Get-AppxPackage *audioremote* | Select-Object -First 1
if (-not $pkg) {
  Write-Host "MSIX パッケージが見つかりません。先に sideload してください:" -ForegroundColor Red
  Write-Host "  pwsh -NoProfile -File packaging\msix\build-msix.ps1 -Install" -ForegroundColor Red
  Read-Host "Enter で閉じる"
  exit 1
}

Write-Host "対象     : $($pkg.PackageFullName)"
Write-Host "レポート : $report"
Write-Host ""
Write-Host "検証中... アプリが自動で起動・終了します。数分かかります。" -ForegroundColor Yellow
Write-Host ""

if (Test-Path $report) { Remove-Item $report -Force }

& $appcert reset | Out-Null
& $appcert test -apptype windowsstoreapp -packagefullname $pkg.PackageFullName -reportoutputpath $report

Write-Host ""
if (Test-Path $report) {
  Write-Host "完了: レポートを出力しました" -ForegroundColor Green
  Write-Host "  $report"
  try {
    [xml]$x = Get-Content $report
    $overall = $x.REPORT.OVERALL_RESULT
    if ($overall) {
      Write-Host "総合判定: $overall" -ForegroundColor $(if ($overall -eq 'PASS') { 'Green' } else { 'Yellow' })
    }
    # 提出を止めるのは OPTIONAL=FALSE の FAIL だけ。そこだけ抜いて見せる。
    $blocking = $x.SelectNodes('//*[@RESULT="FAIL"]') |
      Where-Object { $_.OPTIONAL -ne 'True' } |
      ForEach-Object { $_.NAME } | Where-Object { $_ }
    if ($blocking) {
      Write-Host "提出をブロックする FAIL:" -ForegroundColor Red
      $blocking | ForEach-Object { Write-Host "  - $_" -ForegroundColor Red }
    } else {
      Write-Host "提出をブロックする FAIL（OPTIONAL=FALSE）はありません。" -ForegroundColor Green
    }
  } catch {
    Write-Host "レポートの解析に失敗しました。XML を直接開いてください。" -ForegroundColor Yellow
  }
} else {
  Write-Host "レポートが生成されませんでした（上のログを確認）" -ForegroundColor Red
}

Write-Host ""
Read-Host "Enter で閉じる"
