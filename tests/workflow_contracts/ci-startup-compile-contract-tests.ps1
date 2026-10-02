$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$script:MockCimProcesses = @()
$script:MockProcesses = @{}
$script:CimQueryCount = 0
$script:ProcessQueryIds = [System.Collections.Generic.List[int]]::new()
$script:EventOrder = [System.Collections.Generic.List[string]]::new()
$script:TestRoot = Join-Path ([System.IO.Path]::GetTempPath()) (
    'axinite-startup-contract-' + [Guid]::NewGuid().ToString('N')
)
[void](New-Item -ItemType Directory -Path $script:TestRoot)

function Get-CimInstance {
    [CmdletBinding()]
    param([Parameter(Position = 0)][string] $ClassName)

    [void]($script:CimQueryCount++)
    [void]$script:EventOrder.Add('cim')
    if ($ClassName -ne 'Win32_Process') {
        throw "Unexpected CIM class: $ClassName"
    }
    foreach ($item in $script:MockCimProcesses) {
        Write-Output $item
    }
}

function Get-Process {
    [CmdletBinding()]
    param([Parameter(Mandatory)][int] $Id)

    [void]$script:ProcessQueryIds.Add($Id)
    [void]$script:EventOrder.Add("process:$Id")
    if ($script:MockProcesses.ContainsKey($Id)) {
        $script:MockProcesses[$Id]
    }
}

function Assert-True {
    param([bool] $Condition, [string] $Message)
    if (-not $Condition) {
        throw $Message
    }
}

function Assert-Equal {
    param([object] $Expected, [object] $Actual, [string] $Message)
    if ($Expected -ne $Actual) {
        throw "$Message Expected '$Expected'; got '$Actual'."
    }
}

function Assert-SequenceEqual {
    param([object[]] $Expected, [object[]] $Actual, [string] $Message)
    $expectedText = @($Expected | ForEach-Object { [string] $_ }) -join '|'
    $actualText = @($Actual | ForEach-Object { [string] $_ }) -join '|'
    Assert-Equal -Expected $expectedText -Actual $actualText -Message $Message
}

function Find-LineIndex {
    param([object[]] $Lines, [string] $Prefix)
    for ($index = 0; $index -lt $Lines.Count; $index++) {
        if (([string] $Lines[$index]).StartsWith($Prefix)) {
            return $index
        }
    }
    return -1
}

function New-CimProcess {
    param(
        [int] $ProcessId,
        [int] $ParentProcessId,
        [string] $Name,
        [AllowNull()][string] $CommandLine
    )

    [pscustomobject] @{
        ProcessId       = $ProcessId
        ParentProcessId = $ParentProcessId
        Name            = $Name
        CommandLine     = $CommandLine
    }
}

function New-TestDirectory {
    param([string] $Name)

    $path = Join-Path $script:TestRoot $Name
    [void](New-Item -ItemType Directory -Path $path -Force)
    return $path
}

function New-HiddenLock {
    param([string] $Path, [int] $AgeSeconds)

    [void](New-Item -ItemType Directory -Path (Split-Path -Parent $Path) -Force)
    [System.IO.File]::WriteAllText($Path, '')
    [System.IO.File]::SetLastWriteTimeUtc(
        $Path,
        [DateTime]::UtcNow.AddSeconds(-$AgeSeconds)
    )
    [System.IO.File]::SetAttributes($Path, [System.IO.FileAttributes]::Hidden)
}

function Reset-Mocks {
    $script:MockCimProcesses = @()
    $script:MockProcesses = @{}
    $script:CimQueryCount = 0
    $script:ProcessQueryIds = [System.Collections.Generic.List[int]]::new()
    $script:EventOrder = [System.Collections.Generic.List[string]]::new()
}

function Test-ProcessDiscoveryAndDiagnosticOrder {
    Reset-Mocks
    $cargo = [System.Diagnostics.Process]::GetCurrentProcess()
    $cargoId = $cargo.Id
    $script:MockCimProcesses = @(
        (New-CimProcess 51003 51002 'rustc' 'rustc --deep-child'),
        (New-CimProcess 51999 1 'unrelated' 'unrelated --ignore'),
        (New-CimProcess $cargoId 1 'cargo' 'cargo test --test trybuild'),
        (New-CimProcess 51002 $cargoId 'rustc' 'rustc --child'),
        (New-CimProcess 51004 51003 'rustc' $null)
    )

    $lookupOutput = @(
        Get-CompileProcessIds -CargoProcessId $cargoId -Processes $script:MockCimProcesses
    )
    Assert-Equal -Expected 1 -Actual $lookupOutput.Count `
        -Message 'Process discovery must emit only its tracked-ID lookup.'
    Assert-True -Condition ($lookupOutput[0] -is [hashtable]) `
        -Message 'Process discovery must return a hashtable.'
    $tracked = $lookupOutput[0]
    foreach ($id in @($cargoId, 51002, 51003, 51004)) {
        Assert-True -Condition $tracked.ContainsKey($id) `
            -Message "Descendant process $id must be tracked."
    }
    Assert-True -Condition (-not $tracked.ContainsKey(51999)) `
        -Message 'Unrelated process IDs must not be tracked.'

    $script:MockProcesses[$cargoId] = [pscustomobject] @{ CPU = 0.5; WorkingSet64 = 4096 }
    $script:MockProcesses[51002] = [pscustomobject] @{ CPU = 1.0; WorkingSet64 = 8192 }
    $script:MockProcesses[51003] = [pscustomobject] @{ CPU = 1.5; WorkingSet64 = 12288 }

    $workspace = New-TestDirectory 'snapshot-workspace'
    $lockRoot = Join-Path $workspace 'target/tests/trybuild'
    $lockPaths = @(
        (Join-Path $lockRoot '.lock'),
        (Join-Path $lockRoot 'nested/.lock')
    )
    New-HiddenLock -Path $lockPaths[0] -AgeSeconds 600
    New-HiddenLock -Path $lockPaths[1] -AgeSeconds 900

    $stdoutPath = Join-Path $script:TestRoot 'nested.stdout.log'
    $stderrPath = Join-Path $script:TestRoot 'nested.stderr.log'
    [System.IO.File]::WriteAllLines(
        $stdoutPath,
        [string[]] @(1..20 | ForEach-Object { "stdout-$_" })
    )
    [System.IO.File]::WriteAllLines(
        $stderrPath,
        [string[]] @(1..20 | ForEach-Object { "stderr-$_" })
    )

    $output = @(
        Write-CompileSnapshot -CargoProcess $cargo -Workspace $workspace `
            -StdoutPath $stdoutPath -StderrPath $stderrPath
    )

    Assert-Equal -Expected 1 -Actual $script:CimQueryCount `
        -Message 'A compile snapshot must obtain exactly one CIM process snapshot.'
    Assert-Equal -Expected 'cim' -Actual $script:EventOrder[0] `
        -Message 'The single CIM query must be the first process-query operation.'
    Assert-True -Condition (-not $script:ProcessQueryIds.Contains(51999)) `
        -Message 'Unrelated processes must not be queried.'

    Assert-True -Condition ($output[0] -like 'COMPILE_SNAPSHOT_UTC=*') `
        -Message 'The timestamp must lead the diagnostic output.'
    $processLines = @($output | Where-Object { $_ -like 'COMPILE_PROCESS *' })
    $processIds = @(
        $processLines | ForEach-Object {
            [regex]::Match($_, '^COMPILE_PROCESS pid=(\d+)').Groups[1].Value
        }
    )
    Assert-SequenceEqual -Expected @('51003', "$cargoId", '51002', '51004') `
        -Actual $processIds -Message 'Tracked process records must retain CIM snapshot order.'
    $vanished = $processLines | Where-Object { $_ -like 'COMPILE_PROCESS pid=51004 *' }
    Assert-True -Condition ($vanished -match 'cpu_s=unknown working_set=unknown') `
        -Message 'Vanished processes must report unknown CPU and working set values.'
    Assert-True -Condition ($vanished.EndsWith('command=')) `
        -Message 'A null command line must become an empty command value.'

    $lockIndexes = @(
        for ($index = 0; $index -lt $output.Count; $index++) {
            if ([string] $output[$index] -like 'TRYBUILD_LOCK path=*') { $index }
        }
    )
    Assert-Equal -Expected 2 -Actual $lockIndexes.Count `
        -Message 'The snapshot must include both nested lock files.'
    Assert-True -Condition ($lockIndexes[0] -gt ($processLines.Count)) `
        -Message 'Lock records must follow all process records.'
    $stdoutMarker = Find-LineIndex -Lines $output -Prefix "NESTED_CARGO_OUTPUT path=$stdoutPath"
    $stderrMarker = Find-LineIndex -Lines $output -Prefix "NESTED_CARGO_OUTPUT path=$stderrPath"
    Assert-True -Condition ($stdoutMarker -gt $lockIndexes[-1]) `
        -Message 'Cargo logs must follow trybuild lock records.'
    Assert-True -Condition ($stderrMarker -gt $stdoutMarker) `
        -Message 'Stdout must be emitted before stderr.'
    Assert-SequenceEqual -Expected @(6..20 | ForEach-Object { "stdout-$_" }) `
        -Actual @($output[($stdoutMarker + 1)..($stderrMarker - 1)]) `
        -Message 'Stdout output must contain only its final 15 lines.'
    Assert-SequenceEqual -Expected @(6..20 | ForEach-Object { "stderr-$_" }) `
        -Actual @($output[($stderrMarker + 1)..($output.Count - 1)]) `
        -Message 'Stderr output must contain only its final 15 lines.'
}

function Test-CommandLineBoundaries {
    Reset-Mocks
    $rows = @(
        (New-CimProcess 52001 1 'null-command' $null),
        (New-CimProcess 52002 1 'two-hundred-forty' ('x' * 240)),
        (New-CimProcess 52003 1 'two-hundred-forty-one' ('y' * 241))
    )
    $tracked = @{}
    foreach ($id in @(52001, 52002, 52003)) {
        $tracked[$id] = $true
        $script:MockProcesses[$id] = [pscustomobject] @{ CPU = 1; WorkingSet64 = 2048 }
    }

    $lines = @(Write-CompileProcessSnapshot -Processes $rows -TrackedProcessIds $tracked)
    $nullLine = $lines | Where-Object { $_ -like 'COMPILE_PROCESS pid=52001 *' }
    $line240 = $lines | Where-Object { $_ -like 'COMPILE_PROCESS pid=52002 *' }
    $line241 = $lines | Where-Object { $_ -like 'COMPILE_PROCESS pid=52003 *' }
    Assert-True -Condition $nullLine.EndsWith('command=') `
        -Message 'Null command lines must be converted to empty strings.'
    $command240 = $line240.Substring($line240.IndexOf(' command=') + 9)
    $command241 = $line241.Substring($line241.IndexOf(' command=') + 9)
    Assert-Equal -Expected 240 -Actual $command240.Length `
        -Message 'A 240-character command line must not be truncated.'
    Assert-Equal -Expected ('y' * 240 + '...') -Actual $command241 `
        -Message 'A 241-character command line must keep 240 characters and an ellipsis.'
}

function Test-TrybuildLockSnapshots {
    $workspace = New-TestDirectory 'lock-workspace'
    $missingRoot = @(Write-TrybuildLockSnapshot -Workspace $workspace)
    Assert-SequenceEqual -Expected @('TRYBUILD_LOCK=absent') -Actual $missingRoot `
        -Message 'A missing trybuild root must be reported as absent.'

    $lockRoot = Join-Path $workspace 'target/tests/trybuild'
    [void](New-Item -ItemType Directory -Path $lockRoot -Force)
    $noLocks = @(Write-TrybuildLockSnapshot -Workspace $workspace)
    Assert-SequenceEqual -Expected @('TRYBUILD_LOCK=absent') -Actual $noLocks `
        -Message 'A trybuild root with no lock files must be reported as absent.'

    $firstLock = Join-Path $lockRoot '.lock'
    $secondLock = Join-Path $lockRoot 'nested/.lock'
    New-HiddenLock -Path $firstLock -AgeSeconds 600
    New-HiddenLock -Path $secondLock -AgeSeconds 900
    $locks = @(Write-TrybuildLockSnapshot -Workspace $workspace)
    Assert-Equal -Expected 2 -Actual $locks.Count `
        -Message 'Recursive discovery must include multiple hidden lock files.'
    foreach ($lock in $locks) {
        if ($lock -match '^TRYBUILD_LOCK path=(?<path>.+) age_s=(?<age>\d+)$') {
            $age = [int] $Matches.age
            if ($Matches.path -eq $firstLock) {
                Assert-True -Condition ($age -ge 590 -and $age -le 610) `
                    -Message 'The first lock age must use its UTC write time.'
            } elseif ($Matches.path -eq $secondLock) {
                Assert-True -Condition ($age -ge 890 -and $age -le 910) `
                    -Message 'The nested lock age must use its UTC write time.'
            } else {
                throw "Unexpected lock path: $($Matches.path)"
            }
        } else {
            throw "Unexpected lock diagnostic: $lock"
        }
    }
}

function Test-NestedCargoOutput {
    $missing = @(Write-NestedCargoOutput `
        -StdoutPath (Join-Path $script:TestRoot 'missing.stdout') `
        -StderrPath (Join-Path $script:TestRoot 'missing.stderr'))
    Assert-Equal -Expected 0 -Actual $missing.Count `
        -Message 'Missing Cargo log files must be skipped.'

    $emptyStdout = Join-Path $script:TestRoot 'empty.stdout'
    [System.IO.File]::WriteAllText($emptyStdout, '')
    $empty = @(Write-NestedCargoOutput `
        -StdoutPath $emptyStdout `
        -StderrPath (Join-Path $script:TestRoot 'still-missing.stderr'))
    Assert-SequenceEqual -Expected @("NESTED_CARGO_OUTPUT path=$emptyStdout") `
        -Actual $empty -Message 'An empty existing log must retain its marker.'

    $stdoutPath = Join-Path $script:TestRoot 'tail.stdout'
    $stderrPath = Join-Path $script:TestRoot 'tail.stderr'
    [System.IO.File]::WriteAllLines(
        $stdoutPath,
        [string[]] @(1..20 | ForEach-Object { "stdout-$_" })
    )
    [System.IO.File]::WriteAllLines(
        $stderrPath,
        [string[]] @(1..20 | ForEach-Object { "stderr-$_" })
    )
    $output = @(Write-NestedCargoOutput -StdoutPath $stdoutPath -StderrPath $stderrPath)
    Assert-Equal -Expected "NESTED_CARGO_OUTPUT path=$stdoutPath" -Actual $output[0] `
        -Message 'Stdout must be the first existing log.'
    Assert-SequenceEqual -Expected @(6..20 | ForEach-Object { "stdout-$_" }) `
        -Actual @($output[1..15]) -Message 'Only the final 15 stdout lines must be emitted.'
    Assert-Equal -Expected "NESTED_CARGO_OUTPUT path=$stderrPath" -Actual $output[16] `
        -Message 'Stderr must follow stdout.'
    Assert-SequenceEqual -Expected @(6..20 | ForEach-Object { "stderr-$_" }) `
        -Actual @($output[17..31]) -Message 'Only the final 15 stderr lines must be emitted.'
}

try {
    $repositoryRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
    $launcherPath = Join-Path $repositoryRoot 'scripts/ci-startup-compile-contract.ps1'
    $parseErrors = $null
    $tokens = $null
    $ast = [System.Management.Automation.Language.Parser]::ParseFile(
        $launcherPath,
        [ref] $tokens,
        [ref] $parseErrors
    )
    Assert-Equal -Expected 0 -Actual $parseErrors.Count `
        -Message 'The launcher must parse before function extraction.'

    $requiredNames = @(
        'Get-CompileProcessIds',
        'Write-CompileProcessSnapshot',
        'Write-TrybuildLockSnapshot',
        'Write-NestedCargoOutput',
        'Write-CompileSnapshot'
    )
    $definitions = @(
        $ast.FindAll(
            { param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst] },
            $true
        )
    )
    foreach ($name in $requiredNames) {
        $matches = @($definitions | Where-Object { $_.Name -eq $name })
        Assert-Equal -Expected 1 -Actual $matches.Count `
            -Message "The launcher must define $name exactly once."
        . ([scriptblock]::Create($matches[0].Extent.Text))
    }

    Test-ProcessDiscoveryAndDiagnosticOrder
    Test-CommandLineBoundaries
    Test-TrybuildLockSnapshots
    Test-NestedCargoOutput
    Write-Output 'PASS: startup compile-contract PowerShell behaviour tests'
} finally {
    Remove-Item -LiteralPath $script:TestRoot -Recurse -Force -ErrorAction SilentlyContinue
}
