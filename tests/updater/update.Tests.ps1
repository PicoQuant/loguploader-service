<#
  Pester tests for updater/update.ps1 — the pure decision helpers only (no network, no
  installer). `is-newer` semver precedence is covered by Rust unit tests in src/upgrade.rs;
  here we exercise release/asset selection and the version-compare fallback.

  Run:  Invoke-Pester -Path tests/updater
#>

BeforeAll {
    $repoRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
    . (Join-Path $repoRoot 'updater/update.ps1')
    $fixtures = Join-Path $PSScriptRoot 'fixtures'
    $latest = Get-Content (Join-Path $fixtures 'releases-latest.json') -Raw | ConvertFrom-Json
    $list = Get-Content (Join-Path $fixtures 'releases-list.json') -Raw | ConvertFrom-Json
}

Describe 'Select-Release' {
    It 'stable picks the /releases/latest object' {
        (Select-Release -Channel 'stable' -LatestRelease $latest).tag_name | Should -Be 'v2.0.0'
    }

    It 'beta picks the newest prerelease' {
        (Select-Release -Channel 'beta' -ReleaseList $list).tag_name | Should -Be 'v2.0.0-beta.6'
    }

    It 'beta never crosses to a newer non-prerelease' {
        $picked = Select-Release -Channel 'beta' -ReleaseList $list
        $picked.prerelease | Should -BeTrue
        $picked.tag_name | Should -Not -Be 'v2.0.0'
    }

    It 'beta ignores a draft even when it is the newest' {
        (Select-Release -Channel 'beta' -ReleaseList $list).tag_name | Should -Not -Be 'v2.1.0-beta.1-draft'
    }

    It 'beta returns $null when there is no prerelease' {
        $stableOnly = @($list | Where-Object { -not $_.prerelease })
        Select-Release -Channel 'beta' -ReleaseList $stableOnly | Should -BeNullOrEmpty
    }
}

Describe 'Select-Asset' {
    It 'picks the product-correct beta installer + its .sha256' {
        $rel = Select-Release -Channel 'beta' -ReleaseList $list
        $a = Select-Asset -Release $rel -ProductTitle 'Luminosa' -Channel 'beta'
        $a.Installer.name | Should -Be 'Luminosa.Log.Uploader.Beta.Setup.exe'
        $a.Sha.name | Should -Be 'Luminosa.Log.Uploader.Beta.Setup.exe.sha256'
    }

    It 'picks Solira, not Luminosa, for a Solira box' {
        $rel = Select-Release -Channel 'beta' -ReleaseList $list
        (Select-Asset -Release $rel -ProductTitle 'Solira' -Channel 'beta').Installer.name |
            Should -Be 'Solira.Log.Uploader.Beta.Setup.exe'
    }

    It 'stable does not match a Beta asset' {
        $rel = Select-Release -Channel 'beta' -ReleaseList $list
        Select-Asset -Release $rel -ProductTitle 'Luminosa' -Channel 'stable' | Should -BeNullOrEmpty
    }

    It 'matches the dotted stable asset name' {
        $a = Select-Asset -Release $latest -ProductTitle 'Luminosa' -Channel 'stable'
        $a.Installer.name | Should -Be 'Luminosa.Log.Uploader.Setup.exe'
    }
}

Describe 'Compare-VersionFallback' {
    It '<local> -> <remote> is <expected>' -TestCases @(
        @{ local = '2.0.0-beta.5'; remote = '2.0.0-beta.6'; expected = $true }
        @{ local = '2.0.0-beta.6'; remote = '2.0.0-beta.6'; expected = $false }
        @{ local = '2.0.0';        remote = '2.0.0-beta.6'; expected = $false }  # no downgrade
        @{ local = '2.0.0-beta.6'; remote = '2.0.0';        expected = $true }
        @{ local = '';             remote = '2.0.0-beta.1'; expected = $true }
        @{ local = '2.0.0';        remote = '1.11.0';       expected = $false }
        @{ local = '2.0.0';        remote = '2.1.0';        expected = $true }
    ) {
        Compare-VersionFallback -Local $local -Remote $remote | Should -Be $expected
    }
}

Describe 'Test-ShouldUpdate' {
    It 'falls back to version compare when no exe is present' {
        Test-ShouldUpdate -Exe $null -RemoteVersion '2.0.0-beta.6' -LocalVersion '2.0.0-beta.5' | Should -BeTrue
        Test-ShouldUpdate -Exe $null -RemoteVersion '2.0.0-beta.5' -LocalVersion '2.0.0-beta.5' | Should -BeFalse
    }
}
