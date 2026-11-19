<#
.SYNOPSIS
  Play a TAS script in the real Peggle Deluxe (your Steam copy, native Windows) and capture the run.

.DESCRIPTION
  For the duration of the run this script:
    - puts the playback DLL (play/bin/bass.dll) in the game folder; the game's own bass.dll is kept as
      bass_orig.dll and every sound call is forwarded to it
    - swaps in a fresh game profile (C:\ProgramData\Steam\Peggle\userdata), so every run starts the same way
    - turns the game's "3D acceleration" option off (registry Is3D = 0): with 3D on, Direct3D switches the
      FPU to 24-bit precision and the game no longer matches the simulator bit for bit
  and restores all three afterwards (also on Ctrl+C or error).
  Steam must be running. The game window appears; the run is driven inside the game, not with your mouse
  (except -Video, which glides the OS cursor to each shot's aim point for the recording).

  Outputs in -Out: harness.txt (driver log), oracle.bin (state dumps for `game_check`),
  peggle_oracle_info.txt, script.txt, and with -Video run.mp4 + sound.txt/snd/ (mixed by tas/sound_mix.py).

.EXAMPLE
  powershell -ExecutionPolicy Bypass -File play\play.ps1 -Script plans\0_0.txt -Seed 12 -Out runs\1-1
#>
param(
  [Parameter(Mandatory = $true)][string]$Script,
  [Parameter(Mandatory = $true)][int]$Seed,
  [Parameter(Mandatory = $true)][string]$Out,
  [int]$Speed = 8,            # game clock multiplier (1 = real time; forced to 1 with -Video)
  [int]$Progress = 55,        # profile progress (levels unlocked) set at each `level` command
  [int]$TimeoutSec = 1800,
  [string]$Game = "",         # Peggle Deluxe folder (default: found through Steam)
  [switch]$Video,             # record run.mp4 (needs ffmpeg on PATH) and the sound log
  [switch]$Fast,              # fast forward (right mouse button held) and human-timed clicks, as the plans expect
  [switch]$NoFF               # with -Fast: human-timed clicks without fast forward (flipper plans, one-shot videos)
)
$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
$Dll = Join-Path $Root "play\bin\bass.dll"
$ProfileSrc = Join-Path $Root "play\profile\users.dat"
$RegKey = "HKCU:\Software\Steam\Peggle"
$UserData = "C:\ProgramData\Steam\Peggle\userdata"
# Peggle.exe is Steam's launcher: it starts the game itself as popcapgame1.exe (from C:\ProgramData\PopCap Games)
function Get-GameProcs { Get-Process Peggle, popcapgame* -ErrorAction SilentlyContinue }

function Find-Steam {
  $p = (Get-ItemProperty "HKCU:\Software\Valve\Steam" -ErrorAction SilentlyContinue).SteamPath
  if (-not $p) { throw "Steam not found (HKCU\Software\Valve\Steam\SteamPath)" }
  return ($p -replace "/", "\")
}
function Find-Game([string]$steam) {
  $libs = @($steam)
  $vdf = Join-Path $steam "steamapps\libraryfolders.vdf"
  if (Test-Path $vdf) { foreach ($m in [regex]::Matches((Get-Content $vdf -Raw), '"path"\s+"([^"]+)"')) { $libs += ($m.Groups[1].Value -replace "\\\\", "\") } }
  foreach ($l in $libs) { $g = Join-Path $l "steamapps\common\Peggle Deluxe"; if (Test-Path (Join-Path $g "main.pak")) { return $g } }
  throw "Peggle Deluxe not found in any Steam library: install it in Steam, or pass -Game"
}

if (-not (Test-Path $Dll)) { throw "missing ${Dll}: run ./bootstrap.sh (or play/build.sh) first" }
if (-not (Get-Process steam -ErrorAction SilentlyContinue)) { throw "Steam is not running: start Steam and log in first" }
$Steam = Find-Steam
if (-not $Game) { $Game = Find-Game $Steam }
if ($Video) { $Speed = 1; if (-not (Get-Command ffmpeg -ErrorAction SilentlyContinue)) { throw "-Video needs ffmpeg on PATH (winget install ffmpeg)" } }
if (Get-GameProcs) { throw "Peggle is already running: close it first" }

New-Item -ItemType Directory -Force -Path $Out | Out-Null
$Out = (Resolve-Path $Out).Path
Copy-Item $Script (Join-Path $Out "script.txt") -Force
Remove-Item -Force -ErrorAction SilentlyContinue (Join-Path $Out "oracle.bin"), (Join-Path $Out "harness.txt"), (Join-Path $Out "run.mp4")

$bass = Join-Path $Game "bass.dll"; $orig = Join-Path $Game "bass_orig.dll"
$backup = "$UserData.pegleg-backup"
$hadUserData = (Test-Path $UserData) -or (Test-Path $backup)
$reg = Get-ItemProperty $RegKey -ErrorAction SilentlyContinue
$had3D = $reg -and ($reg.PSObject.Properties.Name -contains "Is3D"); $old3D = if ($had3D) { $reg.Is3D } else { $null }
$ff = $null
try {
  # 1. playback DLL (a leftover bass_orig.dll means an earlier run was interrupted: keep that original)
  if (-not (Test-Path $orig)) { Copy-Item $bass $orig }
  Copy-Item $Dll $bass -Force
  # 2. fresh profile
  if (Test-Path $UserData) { if (Test-Path $backup) { Remove-Item -Recurse -Force $UserData } else { Rename-Item $UserData $backup } }
  New-Item -ItemType Directory -Force -Path $UserData | Out-Null
  Copy-Item $ProfileSrc (Join-Path $UserData "users.dat")
  # 3. 3D acceleration off
  if (-not (Test-Path $RegKey)) { New-Item -Path $RegKey -Force | Out-Null }
  Set-ItemProperty $RegKey -Name Is3D -Value 0 -Type DWord

  $envs = @{
    PEGGLE_SCRIPT = (Join-Path $Out "script.txt"); PEGGLE_LOG = (Join-Path $Out "oracle.bin"); PEGGLE_HLOG = (Join-Path $Out "harness.txt")
    PEGGLE_OUT = $Out; PEGGLE_RNG_SEED = "$Seed"; PEGGLE_PROGRESS = "$Progress"; PEGGLE_SPEED = "$Speed"; PEGGLE_FRMLOG = "1"
    PEGGLE_SOUNDLOG = $(if ($Video) { "1" } else { "0" })
    PEGGLE_FF = $(if ($Fast -and -not $NoFF) { "1" } else { "0" }); PEGGLE_CLICKS = $(if ($Fast) { "1" } else { "0" }); PEGGLE_HUMANFIRE = $(if ($Fast) { "1" } else { "0" })
    # the game's launcher loads Steam.dll from Steam's own folder, as when Steam starts it
    PATH = "$Steam;$env:PATH"; SteamAppId = "3480"; SteamGameId = "3480"
  }
  $psi = New-Object System.Diagnostics.ProcessStartInfo (Join-Path $Game "Peggle.exe")
  $psi.WorkingDirectory = $Game; $psi.UseShellExecute = $false
  foreach ($k in $envs.Keys) { $psi.EnvironmentVariables[$k] = $envs[$k] }
  $t0 = Get-Date
  [System.Diagnostics.Process]::Start($psi) | Out-Null

  if ($Video) {  # record the game window's client area (800x600) once it is up
    $title = $null
    while (-not $title -and (Get-GameProcs) -and ((Get-Date) - $t0).TotalSeconds -lt 60) {
      Start-Sleep -Milliseconds 100; $title = (Get-GameProcs | Where-Object { $_.MainWindowTitle } | Select-Object -First 1).MainWindowTitle
    }
    if ($title) {
      $fpsi = New-Object System.Diagnostics.ProcessStartInfo "ffmpeg"
      $fpsi.Arguments = "-loglevel error -y -f gdigrab -framerate 50 -draw_mouse 1 -i title=`"$title`" -c:v libx264 -preset ultrafast -qp 0 -pix_fmt yuv444p `"$(Join-Path $Out 'run.mp4')`""
      $fpsi.UseShellExecute = $false; $fpsi.RedirectStandardInput = $true
      $ff = [System.Diagnostics.Process]::Start($fpsi)
    }
  }
  # finished when the script's `quit` runs (the game exits), or on timeout
  # (polled every 10 ms with -Video: the stitcher aligns the recording to the moment the game quits)
  $poll = if ($Video) { 10 } else { 250 }
  $hlog = Join-Path $Out "harness.txt"
  $stuck = $null; $last = Get-Date; $size = -1
  while ((Get-GameProcs) -and ((Get-Date) - $t0).TotalSeconds -lt $TimeoutSec) {
    # the DLL logs every frame to harness.txt; if the log doesn't grow for 60 s, the game is stuck (e.g. on an
    # error dialog, which can open behind other windows)
    $n = if (Test-Path $hlog) { (Get-Item $hlog).Length } else { -1 }
    if ($n -ne $size) { $size = $n; $last = Get-Date }
    elseif (((Get-Date) - $last).TotalSeconds -gt 60) { $stuck = $true; break }
    Start-Sleep -Milliseconds $poll
  }
  if ($ff -and -not $ff.HasExited) { $ff.StandardInput.Write("q"); $ff.StandardInput.Close(); $ff.WaitForExit(30000) | Out-Null }
  if ($stuck) { Write-Warning "the game stopped making progress for 60 s (an error dialog may be open): stopping it" }
  elseif (Get-GameProcs) { Write-Warning "timeout: stopping the game" }
} finally {
  if ($ff -and -not $ff.HasExited) { $ff.StandardInput.Write("q"); $ff.StandardInput.Close(); $ff.WaitForExit(30000) | Out-Null }
  Get-GameProcs | Stop-Process -Force -ErrorAction SilentlyContinue
  for ($i = 0; $i -lt 40 -and (Get-GameProcs); $i++) { Start-Sleep -Milliseconds 250 }
  for ($i = 0; (Test-Path $orig) -and $i -lt 20; $i++) {  # the DLL stays locked until the game has fully exited
    try { Copy-Item $orig $bass -Force; Remove-Item $orig -Force } catch { Start-Sleep -Milliseconds 500 }
  }
  if (Test-Path $orig) { Write-Warning "could not restore $bass (still in use): rerun this script, or copy bass_orig.dll over bass.dll by hand" }
  if (Test-Path $backup) { if (Test-Path $UserData) { Remove-Item -Recurse -Force $UserData }; Rename-Item $backup $UserData }
  elseif (-not $hadUserData -and (Test-Path $UserData)) { Remove-Item -Recurse -Force $UserData }
  if ($had3D) { Set-ItemProperty $RegKey -Name Is3D -Value $old3D -Type DWord } else { Remove-ItemProperty $RegKey -Name Is3D -ErrorAction SilentlyContinue }
  # the driver's `quit` skips the game's normal shutdown, which would leave the "still running" flag set
  if (Test-Path $RegKey) { Set-ItemProperty $RegKey -Name InProgress -Value 0 -Type DWord }
}
$h = Join-Path $Out "harness.txt"
$tot = if (Test-Path $h) { (Select-String -Path $h -Pattern "st5 .* total (\d+)" | Select-Object -Last 1) } else { $null }
Write-Output ("done in {0:N0} s; {1}" -f ((Get-Date) - $t0).TotalSeconds, $(if ($tot) { "final total " + $tot.Matches[0].Groups[1].Value } else { "no level-complete total (see $h)" }))
