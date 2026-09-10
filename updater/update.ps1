<#
  update.ps1 - PicoQuant v2 agent auto-updater (channel-aware).

  Runs as SYSTEM from two scheduled tasks the installer registers:
    \PicoQuant\<Product>LogUploader\AutoUpdate       - at boot, +30 s
    \PicoQuant\<Product>LogUploader\AutoUpdateDaily  - daily 03:00

  Behaviour (specs/001-v2-remote-upgrade/contracts/updater-cli.md):
    - a 'stable' install follows GitHub /releases/latest (prereleases excluded - v1 behaviour)
    - a 'beta'   install follows the newest -beta.N prerelease
    - the two channels never cross (spec 001 FR-005b)
  Integrity: SHA-256 file compare (spec 001 C2 - no code-signing cert yet).

  NOT yet implemented (spec 001 US1/US2): post-install health check, automatic rollback,
  SelfHealOnBoot, upgrade mutex. A bad beta stays installed until a newer good beta ships.

  The file is a set of dot-source-safe functions plus a guarded entry point, so
  `. .\update.ps1` loads the helpers for Pester without running the updater.
#>

$ErrorActionPreference = 'Stop'

$script:RepoOwner = 'PicoQuant'
$script:RepoName  = 'loguploader-service'
$script:ApiRoot   = "https://api.github.com/repos/$script:RepoOwner/$script:RepoName"
$script:Http      = @{
    'User-Agent' = "$script:RepoOwner-$script:RepoName-updater"
    'Accept'     = 'application/vnd.github+json'
}
$script:LogPath = $null
$script:EventSource = $null

function Write-UpdaterLog {
    param(
        [string]$Message,
        [ValidateSet('Info', 'Warning', 'Error')][string]$Level = 'Info'
    )
    $line = "[{0}] [{1}] {2}" -f (Get-Date -Format o), $Level.ToUpper(), $Message
    if ($script:LogPath) {
        try { Add-Content -Path $script:LogPath -Value $line } catch { }
    }
    Write-Host $line
    if ($script:EventSource) {
        $entryType = switch ($Level) { 'Error' { 'Error' } 'Warning' { 'Warning' } default { 'Information' } }
        try {
            Write-EventLog -LogName Application -Source $script:EventSource `
                -EntryType $entryType -EventId 1000 -Message $Message
        } catch { }
    }
}

function Get-VersionFromTag {
    param([string]$Tag)
    if (-not $Tag) { return $null }
    if ($Tag.StartsWith('v')) { return $Tag.Substring(1) }
    return $Tag
}

function Get-InstallState {
    <# Resolve exe + product/channel/version from the install root (`{app}`). #>
    param([Parameter(Mandatory)][string]$InstallRoot)

    $exe = $null
    foreach ($name in @('pquploader.exe', 'loguploaderservice.exe')) {
        $candidate = Join-Path $InstallRoot $name
        if (Test-Path $candidate) { $exe = $candidate; break }
    }

    $product = 'luminosa'
    $channel = 'stable'
    $version = ''
    if ($exe) {
        try {
            $j = & $exe version --json 2>$null | Out-String | ConvertFrom-Json
            if ($j.product) { $product = [string]$j.product }
            if ($j.channel) { $channel = [string]$j.channel }
            if ($j.version) { $version = [string]$j.version }
        } catch { }
    }
    if (-not $version) {
        $versionFile = Join-Path $InstallRoot 'VERSION'
        if (Test-Path $versionFile) { $version = (Get-Content $versionFile -Raw).Trim() }
    }

    $product = $product.ToLower()
    $title = if ($product) { $product.Substring(0, 1).ToUpper() + $product.Substring(1) } else { 'Luminosa' }
    [pscustomobject]@{
        Exe          = $exe
        Product      = $product
        Channel      = $channel.ToLower()
        LocalVersion = $version
        ProductTitle = $title
    }
}

function Select-Release {
    <#
      -Channel stable  -> the /releases/latest object (prereleases already excluded).
      -Channel beta    -> the newest prerelease from a /releases list; non-prereleases are
                          ignored entirely so a beta box never crosses to stable (FR-005b).
    #>
    param(
        [Parameter(Mandatory)][ValidateSet('stable', 'beta')][string]$Channel,
        $LatestRelease,
        $ReleaseList
    )
    if ($Channel -eq 'beta') {
        $candidates = @($ReleaseList | Where-Object { $_.prerelease -eq $true -and $_.draft -ne $true })
        if (-not $candidates) { return $null }
        return ($candidates | Sort-Object -Descending {
                if ($_.published_at) { [datetime]$_.published_at }
                elseif ($_.created_at) { [datetime]$_.created_at }
                else { [datetime]::MinValue }
            } | Select-Object -First 1)
    }
    if ($LatestRelease -and $LatestRelease.tag_name) { return $LatestRelease }
    return $null
}

function Select-Asset {
    <# Pick this product+channel's Setup.exe and its .sha256 from a release's assets.
       Published asset names have dots where the on-disk name has spaces (GitHub rewrites
       them), so match loosely: "*<Product>*[Beta*]Setup*.exe". #>
    param(
        [Parameter(Mandatory)]$Release,
        [Parameter(Mandatory)][string]$ProductTitle,
        [Parameter(Mandatory)][ValidateSet('stable', 'beta')][string]$Channel
    )
    $assets = @($Release.assets)
    if ($Channel -eq 'beta') {
        $installer = $assets |
            Where-Object { $_.name -like "*$ProductTitle*Beta*Setup*.exe" } |
            Select-Object -First 1
    } else {
        $installer = $assets |
            Where-Object { $_.name -like "*$ProductTitle*Setup*.exe" -and $_.name -notlike '*Beta*' } |
            Select-Object -First 1
    }
    if (-not $installer) { return $null }

    $sha = $assets | Where-Object { $_.name -eq ($installer.name + '.sha256') } | Select-Object -First 1
    if (-not $sha) {
        $allSha = @($assets | Where-Object { $_.name -like '*.sha256' })
        if ($allSha.Count -eq 1) { $sha = $allSha[0] }  # single-product release - unambiguous
    }
    if (-not $sha) { return $null }

    [pscustomobject]@{ Installer = $installer; Sha = $sha }
}

function Compare-VersionFallback {
    <# Used only when `is-newer` is unavailable (a pre-v2 exe) or can't parse a version.
       Returns $true iff $Remote should be treated as newer than $Local. #>
    param([string]$Local, [string]$Remote)
    if (-not $Local) { return $true }
    if (-not $Remote) { return $false }

    $localCore = $Local -replace '-.*$', ''
    $remoteCore = $Remote -replace '-.*$', ''
    try {
        $a = [Version]$localCore
        $b = [Version]$remoteCore
        if ($b -ne $a) { return ($b -gt $a) }
    } catch {
        return ([string]::Compare($Remote, $Local, $true) -gt 0)
    }
    # Equal core: a prerelease is older than its release; never cross-grade downward.
    $localPre = $Local.Contains('-')
    $remotePre = $Remote.Contains('-')
    if ($remotePre -and -not $localPre) { return $false }
    if ($localPre -and -not $remotePre) { return $true }
    return ([string]::Compare($Remote, $Local, $true) -gt 0)
}

function Test-ShouldUpdate {
    <# $true iff the agent should move to $RemoteVersion. Prefers `pquploader.exe is-newer`
       (full SemVer precedence incl. prerelease); falls back to Compare-VersionFallback. #>
    param([string]$Exe, [Parameter(Mandatory)][string]$RemoteVersion, [string]$LocalVersion)
    if ($Exe -and (Test-Path $Exe)) {
        & $Exe is-newer $RemoteVersion 2>$null | Out-Null
        switch ($LASTEXITCODE) {
            0 { return $true }
            1 { return $false }
            default { }  # 2 = unparseable -> fall through
        }
    }
    return (Compare-VersionFallback -Local $LocalVersion -Remote $RemoteVersion)
}

function Send-UpgradeReport {
    <# Best-effort `pquploader.exe upgrade-report`; never throws, never blocks the outcome. #>
    param(
        [string]$Exe,
        [Parameter(Mandatory)][string]$Outcome,
        [string]$From,
        [string]$To,
        [string]$Cause
    )
    if (-not $Exe -or -not (Test-Path $Exe)) { return }
    $reportArgs = @('upgrade-report', $Outcome)
    if ($From) { $reportArgs += @('--from', $From) }
    if ($To) { $reportArgs += @('--to', $To) }
    if ($Cause) { $reportArgs += @('--cause', $Cause) }
    try { & $Exe @reportArgs 2>$null | Out-Null } catch { }
}

function Invoke-UpdaterMain {
    $installRoot = Split-Path -Parent $PSScriptRoot
    $state = Get-InstallState -InstallRoot $installRoot

    $updateDir = Join-Path $env:ProgramData ("PicoQuant\{0}\update" -f $state.ProductTitle)
    New-Item -ItemType Directory -Force -Path $updateDir | Out-Null
    $script:LogPath = Join-Path $updateDir 'update.log'
    $script:EventSource = "PQUploader{0}Updater" -f $state.ProductTitle
    try {
        if (-not [System.Diagnostics.EventLog]::SourceExists($script:EventSource)) {
            New-EventLog -LogName Application -Source $script:EventSource
        }
    } catch { $script:EventSource = $null }

    Write-UpdaterLog ("updater start - product={0} channel={1} local={2}" -f `
            $state.Product, $state.Channel, $state.LocalVersion)

    # ---- pick the channel's target release ---------------------------------
    try {
        if ($state.Channel -eq 'beta') {
            $list = Invoke-RestMethod -Uri "$script:ApiRoot/releases?per_page=30" -Headers $script:Http -Method Get
            $release = Select-Release -Channel 'beta' -ReleaseList $list
        } else {
            $latest = Invoke-RestMethod -Uri "$script:ApiRoot/releases/latest" -Headers $script:Http -Method Get
            $release = Select-Release -Channel 'stable' -LatestRelease $latest
        }
    } catch {
        Write-UpdaterLog ("could not reach the release channel ({0}); keeping current version" -f `
                $_.Exception.Message) 'Warning'
        return 0
    }
    if (-not $release) {
        Write-UpdaterLog ("no {0}-channel release found; nothing to do" -f $state.Channel) 'Warning'
        return 0
    }

    $remoteVersion = Get-VersionFromTag $release.tag_name
    if (-not (Test-ShouldUpdate -Exe $state.Exe -RemoteVersion $remoteVersion -LocalVersion $state.LocalVersion)) {
        Write-UpdaterLog ("up to date (local={0}, {1}-channel target={2})" -f `
                $state.LocalVersion, $state.Channel, $remoteVersion)
        return 0
    }

    $picked = Select-Asset -Release $release -ProductTitle $state.ProductTitle -Channel $state.Channel
    if (-not $picked) {
        Write-UpdaterLog ("release {0} carries no {1}/{2} installer + .sha256 asset" -f `
                $release.tag_name, $state.ProductTitle, $state.Channel) 'Warning'
        return 0
    }

    # ---- download + verify -----------------------------------------------
    $installerPath = Join-Path $updateDir $picked.Installer.name
    $shaPath = Join-Path $updateDir $picked.Sha.name
    Invoke-WebRequest -Uri $picked.Installer.browser_download_url -Headers $script:Http -OutFile $installerPath
    Invoke-WebRequest -Uri $picked.Sha.browser_download_url -Headers $script:Http -OutFile $shaPath
    Write-UpdaterLog ("downloaded {0} ({1})" -f $picked.Installer.name, $release.tag_name)

    $expected = (((Get-Content $shaPath -Raw).Trim() -split '\s+')[0]).ToLowerInvariant()
    $actual = (Get-FileHash -Path $installerPath -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($expected -ne $actual) {
        Write-UpdaterLog ("SHA-256 mismatch for {0}: expected {1}, got {2}" -f `
                $picked.Installer.name, $expected, $actual) 'Error'
        Send-UpgradeReport -Exe $state.Exe -Outcome 'integrity_failed' -To $remoteVersion -Cause 'sha256 mismatch'
        return 0  # retried next run
    }
    Write-UpdaterLog 'sha256 ok'

    # ---- apply ----------------------------------------------------------
    if ($state.Exe) { try { & $state.Exe stop 2>$null | Out-Null } catch { } }

    Write-UpdaterLog ("running {0} /VERYSILENT" -f $picked.Installer.name)
    $proc = Start-Process -FilePath $installerPath `
        -ArgumentList '/VERYSILENT /SUPPRESSMSGBOXES /NORESTART /SP-' -PassThru -Wait

    $newExe = Join-Path $installRoot 'pquploader.exe'
    if (-not (Test-Path $newExe)) { $newExe = $state.Exe }

    if ($proc.ExitCode -ne 0) {
        Write-UpdaterLog ("installer exited {0}; restarting the version already on disk" -f $proc.ExitCode) 'Error'
        if ($state.Exe) { try { & $state.Exe start 2>$null | Out-Null } catch { } }
        Send-UpgradeReport -Exe $state.Exe -Outcome 'install_failed' -From $state.LocalVersion `
            -To $remoteVersion -Cause ("installer exit {0}" -f $proc.ExitCode)
        return 1
    }

    if ($newExe) { try { & $newExe start 2>$null | Out-Null } catch { } }
    Write-UpdaterLog ("updated {0} -> {1}" -f $state.LocalVersion, $remoteVersion)
    Send-UpgradeReport -Exe $newExe -Outcome 'ok' -From $state.LocalVersion -To $remoteVersion
    return 0
}

if ($MyInvocation.InvocationName -ne '.') {
    try {
        exit (Invoke-UpdaterMain)
    } catch {
        Write-UpdaterLog ("updater error: {0}" -f $_.Exception.Message) 'Error'
        exit 1
    }
}
