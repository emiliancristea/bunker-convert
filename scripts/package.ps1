Param(
    [string[]] $CargoArgs = @()
)

cargo build --release @CargoArgs
cargo run -- security sbom --output target/bunker-convert-sbom.json
cargo run -- security digest --path target/release/bunker-convert --output target/bunker-convert.sha256

Write-Host "Artifacts written to target/"
