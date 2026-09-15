param(
    [Parameter(Mandatory = $true)]
    [string]$BundleRoot,

    [Parameter(Mandatory = $true)]
    [string]$OutputPath,

    [string]$WorkspaceRoot = (Get-Location).Path
)

$ErrorActionPreference = "Stop"

$resolvedBundleRoot = (Resolve-Path -LiteralPath $BundleRoot).Path
$resolvedWorkspaceRoot = (Resolve-Path -LiteralPath $WorkspaceRoot).Path
$installers = @(
    Get-ChildItem -LiteralPath $resolvedBundleRoot -Recurse -File |
        Where-Object { @(".msi", ".exe") -contains $_.Extension.ToLowerInvariant() } |
        Sort-Object FullName
)

if ($installers.Count -eq 0) {
    throw "No Windows installer was produced under $resolvedBundleRoot"
}

$manifest = @(
    $installers | ForEach-Object {
        [ordered]@{
            path = [IO.Path]::GetRelativePath($resolvedWorkspaceRoot, $_.FullName).Replace('\', '/')
            length_bytes = $_.Length
            sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $_.FullName).Hash.ToLowerInvariant()
        }
    }
)

ConvertTo-Json -InputObject $manifest -Depth 3 |
    Set-Content -LiteralPath $OutputPath -Encoding utf8
