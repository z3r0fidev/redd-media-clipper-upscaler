<#
.SYNOPSIS
  SC-009 on real footage: the default "reddit-post" GIF with and without gifsicle.

.DESCRIPTION
  For each clip in the CSV, makes the default reddit-post GIF twice with the size search off
  (--budget none, as the automated bench does): once with --no-optimise, once with gifsicle.
  Then scores both against the source with the bundled ffmpeg's libvmaf (the same filter graph
  as rmedia-testkit's vmaf_gif_at). Writes results.csv and prints the totals.

  Pass: at least 15% smaller over the whole set, and no clip loses more than 1 VMAF point.
  Nothing leaves the machine; the footage is only read.

.PARAMETER Cli
  rmedia.exe from the unpacked CLI archive (rmedia-ffmpeg.exe and rmedia-gifsicle.exe sit next to it).

.PARAMETER Clips
  CSV with the header: file,start,duration  (seconds; file is an absolute path).

.PARAMETER Out
  A new, empty folder for the GIFs and results.

.EXAMPLE
  .\Measure-Sc009.ps1 -Cli C:\rmedia-qa\cli\rmedia.exe -Clips C:\rmedia-qa\footage\clips.csv -Out C:\rmedia-qa\sc009
#>
param(
    [Parameter(Mandatory)] [string] $Cli,
    [Parameter(Mandatory)] [string] $Clips,
    [Parameter(Mandatory)] [string] $Out
)
$ErrorActionPreference = 'Stop'

$ffmpeg = Join-Path (Split-Path -Parent $Cli) 'rmedia-ffmpeg.exe'
if (-not (Test-Path $ffmpeg)) { throw "rmedia-ffmpeg.exe not found next to $Cli" }
if ((Test-Path $Out) -and (Get-ChildItem $Out -Force | Select-Object -First 1)) {
    throw "$Out is not empty; pass a new folder"
}
New-Item -ItemType Directory -Force $Out | Out-Null

function Invoke-Gif([string] $Src, [string] $Start, [string] $Dur, [string] $Dir, [switch] $Plain) {
    $argv = @('gif', $Src, '-s', $Start, '-d', $Dur, '--target', 'reddit-post', '--budget', 'none',
        '-o', $Dir, '--json')
    if ($Plain) { $argv += '--no-optimise' }
    $lines = & $Cli @argv
    if ($LASTEXITCODE -ne 0) { throw "rmedia gif exited $LASTEXITCODE on $Src" }
    $done = $lines | ForEach-Object { $_ | ConvertFrom-Json } |
        Where-Object { $_.type -eq 'job_finished' } | Select-Object -First 1
    if ($done.outcome.status -ne 'succeeded') { throw "job $($done.outcome.status) on $Src" }
    $done.outcome
}

function Get-Vmaf([string] $Gif, [string] $Src, [int] $Fps, [string] $Start, [int] $W, [int] $H) {
    $work = Join-Path $Out ('vmaf-' + [guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory $work | Out-Null
    $graph = "[0:v]fps=${Fps},setpts=N/(${Fps}*TB),scale=${W}:${H}:flags=neighbor,format=rgb24,format=yuv444p[d];" +
        "[1:v]fps=${Fps},scale=${W}:${H}:flags=bicubic,format=rgb24,setpts=N/(${Fps}*TB),format=yuv444p[r];" +
        "[d][r]libvmaf=log_fmt=json:log_path=vmaf.json:n_threads=4:shortest=1"
    Push-Location $work
    try {
        & $ffmpeg -hide_banner -nostdin -loglevel error -i $Gif -ss $Start -i $Src -lavfi $graph -f null -
        if ($LASTEXITCODE -ne 0) { throw "libvmaf failed on $Gif" }
        [double] (Get-Content vmaf.json -Raw | ConvertFrom-Json).pooled_metrics.vmaf.mean
    } finally {
        Pop-Location
        Remove-Item -Recurse -Force $work
    }
}

$rows = @()
$n = 0
foreach ($c in Import-Csv $Clips) {
    $n++
    Write-Host "[$n] $($c.file) from $($c.start) s for $($c.duration) s"
    $dir = Join-Path $Out ('{0:D2}' -f $n)
    $p = Invoke-Gif $c.file $c.start $c.duration (Join-Path $dir 'plain') -Plain
    $o = Invoke-Gif $c.file $c.start $c.duration (Join-Path $dir 'gifsicle')
    if ($p.optimiser) { throw "--no-optimise still ran gifsicle on $($c.file)" }
    if (-not $o.optimiser) { throw "gifsicle did not run on $($c.file); check 'rmedia doctor'" }
    $fps = [int][math]::Round([double]$o.fps)
    $vp = Get-Vmaf $p.output $c.file $fps $c.start $p.width $p.height
    $vo = Get-Vmaf $o.output $c.file $fps $c.start $o.width $o.height
    $row = [pscustomobject]@{
        clip        = $c.file
        start       = $c.start
        duration    = $c.duration
        size        = "$($o.width)x$($o.height)@$($o.fps)"
        lossy       = $o.lossy
        plain_bytes = [long]$p.bytes
        opt_bytes   = [long]$o.bytes
        saved_pct   = [math]::Round((1 - $o.bytes / $p.bytes) * 100, 1)
        vmaf_plain  = [math]::Round($vp, 2)
        vmaf_opt    = [math]::Round($vo, 2)
        vmaf_loss   = [math]::Round($vp - $vo, 2)
    }
    $row | Format-List | Out-String | Write-Host
    $rows += $row
}

$rows | Export-Csv -NoTypeInformation (Join-Path $Out 'results.csv')
$plain = ($rows | Measure-Object plain_bytes -Sum).Sum
$opt = ($rows | Measure-Object opt_bytes -Sum).Sum
$saved = (1 - $opt / $plain) * 100
$worst = ($rows | Measure-Object vmaf_loss -Maximum).Maximum
$verdict = if ($saved -ge 15 -and $worst -le 1.0) { 'PASS' } else { 'FAIL' }
Write-Host ("SC-009: {0} clips, {1:N0} -> {2:N0} bytes ({3:N1}% smaller), worst VMAF loss {4:N2}: {5}" -f
    $rows.Count, $plain, $opt, $saved, $worst, $verdict)
