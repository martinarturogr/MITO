param([string]$ContextPath)
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = New-Object System.Text.UTF8Encoding($false)
try {
    $context = Get-Content -LiteralPath $ContextPath -Raw -Encoding UTF8 | ConvertFrom-Json
    $dir = $context.dir
    $original_dir = $dir
    $persist_dir = $context.persist_dir
    $app = $context.app
    $bucket = $context.bucket
    $bucketsdir = $context.bucketsdir
    $version = $context.version
    $architecture = '64bit'
    $global = $false
    $manifest = $context.manifest
    $fname = $context.fname
    function info { param([string]$Message) Write-Output $Message }
    function warn { param([string]$Message) Write-Output "Aviso: $Message" }
    function error { param([string]$Message) throw $Message }
    function abort { param([string]$Message) throw $Message }
    # Like Scoop, the Expand-* helpers accept the archive by pipeline
    # (Get-Item ... | Expand-7zipArchive); FullName binds files and directories by
    # their complete path. Each piped archive is expanded in turn.
    function Invoke-MitoExpand {
        param([string]$Kind, [string]$Name, [string]$Path, [string]$DestinationPath, [string]$ExtractDir, [bool]$Removal)
        if (!$DestinationPath) { $DestinationPath = Split-Path $Path }
        & $context.mito hook-expand $Kind $context.root $dir $Path $DestinationPath $ExtractDir
        if ($LASTEXITCODE -ne 0) { throw "$Name falló" }
        if ($Removal) { Remove-Item -LiteralPath $Path -Force }
    }
    function Expand-7zipArchive {
        [CmdletBinding()]
        param(
            [Parameter(Mandatory=$true, Position=0, ValueFromPipeline=$true, ValueFromPipelineByPropertyName=$true)][Alias('FullName')][string]$Path,
            [Parameter(Position=1)][string]$DestinationPath,
            [string]$ExtractDir = '.',
            [switch]$Removal
        )
        process { Invoke-MitoExpand '7zip' 'Expand-7zipArchive' $Path $DestinationPath $ExtractDir $Removal }
    }
    function Expand-InnoArchive {
        [CmdletBinding()]
        param(
            [Parameter(Mandatory=$true, Position=0, ValueFromPipeline=$true, ValueFromPipelineByPropertyName=$true)][Alias('FullName')][string]$Path,
            [Parameter(Position=1)][string]$DestinationPath,
            [string]$ExtractDir = '{app}',
            [switch]$Removal
        )
        process { Invoke-MitoExpand 'inno' 'Expand-InnoArchive' $Path $DestinationPath $ExtractDir $Removal }
    }
    function Expand-DarkArchive {
        [CmdletBinding()]
        param(
            [Parameter(Mandatory=$true, Position=0, ValueFromPipeline=$true, ValueFromPipelineByPropertyName=$true)][Alias('FullName')][string]$Path,
            [Parameter(Position=1)][string]$DestinationPath,
            [switch]$Removal
        )
        process { Invoke-MitoExpand 'dark' 'Expand-DarkArchive' $Path $DestinationPath '.' $Removal }
    }
    function Expand-MsiArchive {
        [CmdletBinding()]
        param(
            [Parameter(Mandatory=$true, Position=0, ValueFromPipeline=$true, ValueFromPipelineByPropertyName=$true)][Alias('FullName')][string]$Path,
            [Parameter(Position=1)][string]$DestinationPath,
            [string]$ExtractDir = '.',
            [switch]$Removal
        )
        process { Invoke-MitoExpand 'msi' 'Expand-MsiArchive' $Path $DestinationPath $ExtractDir $Removal }
    }
    function Get-HelperPath {
        [CmdletBinding()]
        param([Parameter(Mandatory=$true)][string]$Helper)
        if ($Helper -ne 'Innounp') { throw "Helper no soportado: $Helper" }
        $helperPath = Join-Path $context.root 'core/tools/innounp/innounp.exe'
        if (!(Test-Path -LiteralPath $helperPath -PathType Leaf)) { throw 'Herramienta incompleta: innounp' }
        return $helperPath
    }
    function Invoke-ExternalCommand {
        [CmdletBinding()]
        param(
            [Parameter(Mandatory=$true)][string]$FilePath,
            [string[]]$ArgumentList = @(),
            [string]$LogPath
        )
        $output = & $FilePath @ArgumentList 2>&1
        $code = $LASTEXITCODE
        if ($LogPath) { $output | Out-File -LiteralPath $LogPath -Encoding utf8 }
        if ($code -ne 0) { throw "$FilePath falló ($code): $($output -join [Environment]::NewLine)" }
        return $output
    }
    Set-Location -LiteralPath $dir
    if ($context.file) {
        $installerArgs = @($context.arguments)
        $global:LASTEXITCODE = 0
        & $context.file @installerArgs
        if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    } else {
        & ([scriptblock]::Create($context.script))
    }
} catch {
    [Console]::Error.WriteLine($_.ToString())
    exit 1
}
