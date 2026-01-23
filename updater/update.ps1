$ErrorActionPreference = 'Stop'

$RepoOwner = 'PicoQuant'
$RepoName = 'loguploader-service'
$ServiceName = 'LumiLogUploadService'

$AppDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$InstallRoot = Split-Path -Parent $AppDir
$LocalVersionPath = Join-Path $InstallRoot 'VERSION'

$TempDir = Join-Path $env:ProgramData 'PicoQuant\LuminosaLogUploader\update'
New-Item -ItemType Directory -Force -Path $TempDir | Out-Null

function Get-VersionFromTag($tag) {
  if (!$tag) { return $null }
  if ($tag.StartsWith('v')) { return $tag.Substring(1) }
  return $tag
}

function Compare-SemVer($a, $b) {
  # returns -1 if a<b, 0 if equal, 1 if a>b
  try {
    $va = [Version]$a
    $vb = [Version]$b
    return $va.CompareTo($vb)
  } catch {
    return [String]::Compare($a, $b, $true)
  }
}

$localVersion = ''
if (Test-Path $LocalVersionPath) {
  $localVersion = (Get-Content $LocalVersionPath -Raw).Trim()
}

$api = "https://api.github.com/repos/$RepoOwner/$RepoName/releases/latest"
$headers = @{ 'User-Agent' = "$RepoOwner-$RepoName-updater"; 'Accept' = 'application/vnd.github+json' }
$release = Invoke-RestMethod -Uri $api -Headers $headers -Method Get

$tag = $release.tag_name
$remoteVersion = Get-VersionFromTag $tag
if (!$remoteVersion) { exit 0 }

if ($localVersion -and (Compare-SemVer $localVersion $remoteVersion) -ge 0) {
  exit 0
}

$assets = @($release.assets)
$installerAsset = $assets | Where-Object { $_.name -like '*Setup*.exe' } | Select-Object -First 1
if (-not $installerAsset) {
  $installerAsset = $assets | Where-Object { $_.name -like '*.exe' } | Select-Object -First 1
}
if (-not $installerAsset) { throw 'No installer asset found in latest release.' }

$shaAsset = $assets | Where-Object { $_.name -eq ($installerAsset.name + '.sha256') } | Select-Object -First 1
if (-not $shaAsset) {
  $shaAsset = $assets | Where-Object { $_.name -like '*.sha256' } | Select-Object -First 1
}
if (-not $shaAsset) { throw 'No sha256 asset found in latest release.' }

$installerPath = Join-Path $TempDir $installerAsset.name
$shaPath = Join-Path $TempDir $shaAsset.name

Invoke-WebRequest -Uri $installerAsset.browser_download_url -Headers $headers -OutFile $installerPath
Invoke-WebRequest -Uri $shaAsset.browser_download_url -Headers $headers -OutFile $shaPath

$expectedHash = (Get-Content $shaPath -Raw).Trim().Split(' ')[0]
$actualHash = (Get-FileHash -Path $installerPath -Algorithm SHA256).Hash.ToLowerInvariant()
if ($expectedHash.ToLowerInvariant() -ne $actualHash) {
  throw "SHA256 mismatch for installer. expected=$expectedHash actual=$actualHash"
}

$exe = Join-Path $InstallRoot 'loguploaderservice.exe'
if (Test-Path $exe) {
  try { & $exe stop | Out-Null } catch {}
}

$arguments = '/VERYSILENT /SUPPRESSMSGBOXES /NORESTART /SP-'
$proc = Start-Process -FilePath $installerPath -ArgumentList $arguments -PassThru -Wait
if ($proc.ExitCode -ne 0) {
  throw "Installer failed with exit code $($proc.ExitCode)"
}

if (Test-Path $exe) {
  try { & $exe start | Out-Null } catch {}
}
