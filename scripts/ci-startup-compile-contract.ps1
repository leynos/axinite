$ErrorActionPreference = 'Stop'

$workspace = $env:GITHUB_WORKSPACE
$runnerTemp = $env:RUNNER_TEMP
$stdoutPath = Join-Path $runnerTemp 'startup-compile-contract.stdout.log'
$stderrPath = Join-Path $runnerTemp 'startup-compile-contract.stderr.log'
$arguments = @(
    'test',
    '--features', 'test-helpers',
    '--test', 'trybuild',
    'startup_compile_contracts',
    '--', '--nocapture'
)

function Get-CompileProcessIds {
    <#
    .SYNOPSIS
    Returns the Cargo process ID and every discoverable descendant ID.

    .DESCRIPTION
    Win32_Process rows are not guaranteed to be parent-before-child. The
    fixed-point search revisits the snapshot until an entire pass discovers
    no more descendants. The success stream returns only the tracked-ID
    hashtable; all intermediate work stays local to this function.
    #>
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)]
        [int] $CargoProcessId,

        [Parameter(Mandatory)]
        [AllowEmptyCollection()]
        [object[]] $Processes
    )

    $tracked = @{}
    $tracked[$CargoProcessId] = $true
    $changed = $true

    while ($changed) {
        $changed = $false
        foreach ($item in $processes) {
            $processId = [int] $item.ProcessId
            $parentId = [int] $item.ParentProcessId
            if ($tracked.ContainsKey($parentId) -and -not $tracked.ContainsKey($processId)) {
                $tracked[$processId] = $true
                $changed = $true
            }
        }
    }

    return $tracked
}

function Write-CompileProcessSnapshot {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)]
        [AllowEmptyCollection()]
        [object[]] $Processes,

        [Parameter(Mandatory)]
        [hashtable] $TrackedProcessIds
    )

    foreach ($item in $Processes) {
        if (-not $TrackedProcessIds.ContainsKey([int] $item.ProcessId)) {
            continue
        }

        $processId = [int] $item.ProcessId
        $process = Get-Process -Id $processId -ErrorAction SilentlyContinue
        $cpuSeconds = if ($null -ne $process) { $process.CPU } else { 'unknown' }
        $workingSet = if ($null -ne $process) { $process.WorkingSet64 } else { 'unknown' }
        $commandLine = [string] $item.CommandLine
        if ($commandLine.Length -gt 240) {
            $commandLine = $commandLine.Substring(0, 240) + '...'
        }
        Write-Output (
            'COMPILE_PROCESS pid={0} parent={1} name={2} cpu_s={3} working_set={4} command={5}' -f `
                $item.ProcessId, $item.ParentProcessId, $item.Name, $cpuSeconds, $workingSet, $commandLine
        )
    }
}

function Write-TrybuildLockSnapshot {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)]
        [string] $Workspace
    )

    $trybuildRoot = Join-Path $Workspace 'target\tests\trybuild'
    $locks = @()
    if (Test-Path $trybuildRoot) {
        $locks = @(
            Get-ChildItem -Path $trybuildRoot -Filter '.lock' -File -Force -Recurse `
                -ErrorAction SilentlyContinue
        )
    }
    if ($locks.Count -eq 0) {
        Write-Output 'TRYBUILD_LOCK=absent'
        return
    }

    foreach ($lock in $locks) {
        $age = [DateTime]::UtcNow - $lock.LastWriteTimeUtc
        Write-Output "TRYBUILD_LOCK path=$($lock.FullName) age_s=$([int] $age.TotalSeconds)"
    }
}

function Write-NestedCargoOutput {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)]
        [string] $StdoutPath,

        [Parameter(Mandatory)]
        [string] $StderrPath
    )

    foreach ($path in @($StdoutPath, $StderrPath)) {
        if (Test-Path $path) {
            Write-Output "NESTED_CARGO_OUTPUT path=$path"
            Get-Content -Path $path -Tail 15
        }
    }
}

function Write-CompileSnapshot {
    param(
        [System.Diagnostics.Process] $CargoProcess,
        [string] $Workspace,
        [string] $StdoutPath,
        [string] $StderrPath
    )

    $CargoProcess.Refresh()
    $processes = @(Get-CimInstance Win32_Process)
    $tracked = Get-CompileProcessIds -CargoProcessId $CargoProcess.Id -Processes $processes
    Write-Output "COMPILE_SNAPSHOT_UTC=$([DateTime]::UtcNow.ToString('o'))"
    Write-CompileProcessSnapshot -Processes $processes -TrackedProcessIds $tracked
    Write-TrybuildLockSnapshot -Workspace $Workspace
    Write-NestedCargoOutput -StdoutPath $StdoutPath -StderrPath $StderrPath
}

$cargoProcess = Start-Process -FilePath 'cargo' -ArgumentList $arguments `
    -WorkingDirectory $workspace -NoNewWindow -PassThru `
    -RedirectStandardOutput $stdoutPath -RedirectStandardError $stderrPath

Write-Output 'Startup compile contract started; sampling process and Cargo state every 30 seconds.'
while (-not $cargoProcess.WaitForExit(30000)) {
    Write-CompileSnapshot -CargoProcess $cargoProcess -Workspace $workspace `
        -StdoutPath $stdoutPath -StderrPath $stderrPath
}

$cargoProcess.Refresh()
Write-CompileSnapshot -CargoProcess $cargoProcess -Workspace $workspace `
    -StdoutPath $stdoutPath -StderrPath $stderrPath
Write-Output "STARTUP_COMPILE_CONTRACT_EXIT=$($cargoProcess.ExitCode)"
exit $cargoProcess.ExitCode
