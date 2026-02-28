param(
    [Parameter(Mandatory=$true, Position=0)]
    [string]$Name
)

$dataset = "datasets/$Name.json"
if (-not (Test-Path $dataset)) {
    Write-Error "Dataset not found: $dataset"
    Write-Host "Available datasets:"
    Get-ChildItem datasets/*.json | ForEach-Object { Write-Host "  $($_.BaseName)" }
    exit 1
}

cargo run --release -q -- --dataset $dataset
