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

function Write-CompileSnapshot {
    param([System.Diagnostics.Process] $CargoProcess)

    $CargoProcess.Refresh()
    $processes = @(Get-CimInstance Win32_Process)
    $tracked = @{}
    $tracked[$CargoProcess.Id] = $true
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

    Write-Output "COMPILE_SNAPSHOT_UTC=$([DateTime]::UtcNow.ToString('o'))"
    foreach ($item in $processes | Where-Object { $tracked.ContainsKey([int] $_.ProcessId) }) {
        $process = Get-Process -Id ([int] $item.ProcessId) -ErrorAction SilentlyContinue
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

    $trybuildRoot = Join-Path $workspace 'target\tests\trybuild'
    $locks = @()
    if (Test-Path $trybuildRoot) {
        $locks = @(
            Get-ChildItem -Path $trybuildRoot -Filter '.lock' -File -Force -Recurse `
                -ErrorAction SilentlyContinue
        )
    }
    if ($locks.Count -eq 0) {
        Write-Output 'TRYBUILD_LOCK=absent'
    } else {
        foreach ($lock in $locks) {
            $age = [DateTime]::UtcNow - $lock.LastWriteTimeUtc
            Write-Output "TRYBUILD_LOCK path=$($lock.FullName) age_s=$([int] $age.TotalSeconds)"
        }
    }

    foreach ($path in @($stdoutPath, $stderrPath)) {
        if (Test-Path $path) {
            Write-Output "NESTED_CARGO_OUTPUT path=$path"
            Get-Content -Path $path -Tail 15
        }
    }
}

$cargoProcess = Start-Process -FilePath 'cargo' -ArgumentList $arguments `
    -WorkingDirectory $workspace -NoNewWindow -PassThru `
    -RedirectStandardOutput $stdoutPath -RedirectStandardError $stderrPath

Write-Output 'Startup compile contract started; sampling process and Cargo state every 30 seconds.'
while (-not $cargoProcess.WaitForExit(30000)) {
    Write-CompileSnapshot -CargoProcess $cargoProcess
}

$cargoProcess.Refresh()
Write-CompileSnapshot -CargoProcess $cargoProcess
Write-Output "STARTUP_COMPILE_CONTRACT_EXIT=$($cargoProcess.ExitCode)"
exit $cargoProcess.ExitCode
