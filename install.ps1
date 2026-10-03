# ink installer for Windows - https://github.com/borghei/ink
#
# Usage (PowerShell 5.1 or 7+):
#   irm https://raw.githubusercontent.com/borghei/ink/main/install.ps1 | iex
#
# Environment:
#   $env:INK_VERSION = "v0.11.1"        install a specific release (default: latest)
#   $env:INK_INSTALL_DIR = "C:\tools"  install somewhere else
#                                      (default: %LOCALAPPDATA%\Programs\ink)
#
# Everything runs inside one script block, so a truncated download fails to
# parse instead of running half an install. Errors are reported without
# `exit`, which would close the terminal when run through `iex`.

& {
    $ErrorActionPreference = 'Stop'
    # Invoke-WebRequest is dramatically slower with the progress bar on 5.1.
    $ProgressPreference = 'SilentlyContinue'
    # Windows PowerShell 5.1 may default to TLS 1.0, which GitHub refuses.
    [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12

    $repo = 'borghei/ink'

    function Get-InkArch {
        # Win32_Processor sees the real CPU even from an emulated x64 shell
        # on ARM64 (Architecture 9 = x64, 12 = ARM64).
        try {
            $cpu = Get-CimInstance -ClassName Win32_Processor -ErrorAction Stop | Select-Object -First 1
            if ($cpu.Architecture -eq 12) { return 'arm64' }
            if ($cpu.Architecture -eq 9) { return 'amd64' }
        } catch { }
        $arch = $null
        try {
            $arch = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString()
        } catch { }
        if (-not $arch) {
            $arch = $env:PROCESSOR_ARCHITEW6432
            if (-not $arch) { $arch = $env:PROCESSOR_ARCHITECTURE }
        }
        switch -Regex ($arch) {
            '^(X64|AMD64)$' { return 'amd64' }
            '^(Arm64|ARM64)$' { return 'arm64' }
            default {
                throw "unsupported architecture '$arch'. Prebuilt binaries exist for Windows x64 and ARM64; elsewhere use: cargo install ink-md"
            }
        }
    }

    function Get-File([string]$Url, [string]$OutFile) {
        Invoke-WebRequest -Uri $Url -OutFile $OutFile -UseBasicParsing
    }

    $tmp = Join-Path ([IO.Path]::GetTempPath()) ("ink-install-" + [guid]::NewGuid().ToString('N'))
    try {
        $arch = Get-InkArch

        $version = $env:INK_VERSION
        if ($version) {
            if (-not $version.StartsWith('v')) { $version = "v$version" }
            $base = "https://github.com/$repo/releases/download/$version"
            $label = $version
        } else {
            # The /latest/download redirect avoids the rate-limited GitHub API.
            $base = "https://github.com/$repo/releases/latest/download"
            $label = 'latest'
        }

        $installDir = $env:INK_INSTALL_DIR
        if (-not $installDir) { $installDir = Join-Path $env:LOCALAPPDATA 'Programs\ink' }

        New-Item -ItemType Directory -Force -Path $tmp | Out-Null

        # Checksums first: they double as the list of assets in the release.
        $sumsFile = Join-Path $tmp 'SHA256SUMS'
        try {
            Get-File "$base/SHA256SUMS" $sumsFile
        } catch {
            $hint = ''
            if ($env:INK_VERSION) { $hint = " Does release $version exist? See https://github.com/$repo/releases" }
            throw "could not download $base/SHA256SUMS - $($_.Exception.Message)$hint"
        }
        $sums = @{}
        foreach ($line in Get-Content -LiteralPath $sumsFile) {
            if ($line -match '^([0-9a-fA-F]{64})\s+\*?(.+?)\s*$') { $sums[$Matches[2]] = $Matches[1] }
        }

        $asset = "ink-windows-$arch.exe"
        if (-not $sums.ContainsKey($asset) -and $arch -eq 'arm64') {
            # Releases before v0.9.0 have no native ARM64 build; Windows on ARM
            # runs the x64 one under emulation.
            Write-Host "Release $label has no native ARM64 build; installing the x64 build (runs under emulation)."
            $asset = 'ink-windows-amd64.exe'
        }
        if (-not $sums.ContainsKey($asset)) {
            throw "release $label has no $asset. Build from source instead: cargo install ink-md"
        }

        Write-Host "Installing ink ($label): $asset"
        $exeTmp = Join-Path $tmp 'ink.exe'
        Get-File "$base/$asset" $exeTmp

        Write-Host 'Verifying checksum...'
        $actual = (Get-FileHash -LiteralPath $exeTmp -Algorithm SHA256).Hash
        if ($actual -ine $sums[$asset]) {
            throw "checksum mismatch for $asset`n  expected: $($sums[$asset])`n  actual:   $actual"
        }
        Write-Host 'Checksum OK.'

        New-Item -ItemType Directory -Force -Path $installDir | Out-Null
        $exe = Join-Path $installDir 'ink.exe'
        try {
            Copy-Item -LiteralPath $exeTmp -Destination $exe -Force
        } catch {
            throw "could not write $exe ($($_.Exception.Message)). If ink is running, close it and try again."
        }

        # Add the install dir to the *user* PATH. Read and write the raw
        # registry value so %VARIABLE% entries in it stay unexpanded.
        $onWindows = ($PSVersionTable.PSEdition -eq 'Desktop') -or $IsWindows
        $pathNote = $null
        if ($onWindows) {
            $key = Get-Item -Path 'HKCU:\Environment'
            $userPath = $key.GetValue('Path', '', 'DoNotExpandEnvironmentNames')
            $entries = @($userPath -split ';' | Where-Object { $_ })
            $present = $entries | Where-Object { $_.TrimEnd('\') -ieq $installDir.TrimEnd('\') }
            if (-not $present) {
                $newPath = (@($entries) + $installDir) -join ';'
                Set-ItemProperty -Path 'HKCU:\Environment' -Name 'Path' -Value $newPath -Type ExpandString
                # Setting any user variable through .NET broadcasts
                # WM_SETTINGCHANGE, so newly opened terminals see the new PATH.
                [Environment]::SetEnvironmentVariable('INK_INSTALLER_REFRESH', '1', 'User')
                [Environment]::SetEnvironmentVariable('INK_INSTALLER_REFRESH', $null, 'User')
                $pathNote = "Added $installDir to your user PATH. Open a new terminal to use 'ink'."
            }
            if (-not (($env:Path -split ';') | Where-Object { $_.TrimEnd('\') -ieq $installDir.TrimEnd('\') })) {
                $env:Path = "$env:Path;$installDir"
            }
        }

        $installed = $null
        try { $installed = (& $exe --version) 2>$null } catch { }
        if ($installed) {
            Write-Host "$installed installed to $exe"
        } else {
            Write-Host "ink installed to $exe"
        }
        if ($pathNote) { Write-Host $pathNote }
        Write-Host "Run 'ink --help' to get started."
    } catch {
        Write-Host "error: $($_.Exception.Message)" -ForegroundColor Red
    } finally {
        if (Test-Path -LiteralPath $tmp) { Remove-Item -LiteralPath $tmp -Recurse -Force -ErrorAction SilentlyContinue }
    }
}
