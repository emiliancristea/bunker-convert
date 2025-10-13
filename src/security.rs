use std::fs::File;
use std::io::{BufReader, Read, Write};
use std::path::Path;

use anyhow::{Context, Result};
use cargo_metadata::{Metadata, MetadataCommand};
use serde::Serialize;
use sha2::{Digest, Sha256};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Bom {
    bom_format: &'static str,
    spec_version: &'static str,
    version: u32,
    metadata: BomMetadata,
    components: Vec<Component>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BomMetadata {
    timestamp: String,
    tools: Vec<Tool>,
}

#[derive(Debug, Serialize)]
struct Tool {
    name: &'static str,
    version: &'static str,
}

#[derive(Debug, Serialize)]
struct Component {
    #[serde(rename = "type")]
    component_type: &'static str,
    name: String,
    version: Option<String>,
    purl: Option<String>,
    licenses: Option<Vec<LicenseWrapper>>,
}

#[derive(Debug, Serialize)]
struct LicenseWrapper {
    license: License,
}

#[derive(Debug, Serialize)]
struct License {
    id: String,
}

/// Generate a CycloneDX-style SBOM for the current crate and write it to `output`.
pub fn generate_sbom(output: &Path) -> Result<()> {
    let metadata = MetadataCommand::new()
        .exec()
        .context("Failed to fetch cargo metadata")?;

    write_sbom(&metadata, output)
}

fn write_sbom(metadata: &Metadata, output: &Path) -> Result<()> {
    let timestamp = chrono::Utc::now().to_rfc3339();
    let mut components = Vec::new();
    let root_id = metadata.root_package().map(|pkg| pkg.id.clone());

    for package in &metadata.packages {
        let is_root = root_id
            .as_ref()
            .map(|id| id == &package.id)
            .unwrap_or(false);

        if package.source.is_none() && !is_root {
            // Skip path dependencies outside crates.io to avoid leaking local paths
            continue;
        }

        components.push(Component {
            component_type: "library",
            name: package.name.clone(),
            version: Some(package.version.to_string()),
            purl: Some(format!(
                "pkg:cargo/{name}@{version}",
                name = package.name,
                version = package.version
            )),
            licenses: package.license.as_ref().map(|expr| {
                vec![LicenseWrapper {
                    license: License { id: expr.clone() },
                }]
            }),
        });
    }

    let bom = Bom {
        bom_format: "CycloneDX",
        spec_version: "1.5",
        version: 1,
        metadata: BomMetadata {
            timestamp,
            tools: vec![Tool {
                name: "bunker-convert",
                version: env!("CARGO_PKG_VERSION"),
            }],
        },
        components,
    };

    if let Some(parent) = output.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create SBOM directory: {}", parent.display()))?;
    }

    let file = File::create(output)
        .with_context(|| format!("Failed to create SBOM file: {}", output.display()))?;
    serde_json::to_writer_pretty(file, &bom)
        .with_context(|| format!("Failed to write SBOM JSON: {}", output.display()))?;

    Ok(())
}

/// Compute the SHA256 digest of the file at `path` and return it as a hex string.
pub fn compute_sha256(path: &Path) -> Result<String> {
    let file = File::open(path)
        .with_context(|| format!("Failed to open file for hashing: {}", path.display()))?;
    let mut reader = BufReader::new(file);
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 8192];

    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }

    Ok(format!("{:x}", hasher.finalize()))
}

/// Write the SHA256 digest of `path` into the `output` file.
pub fn write_sha256(path: &Path, output: &Path) -> Result<String> {
    let digest = compute_sha256(path)?;
    if let Some(parent) = output.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create digest directory: {}", parent.display()))?;
    }
    let mut file = File::create(output)
        .with_context(|| format!("Failed to create digest file: {}", output.display()))?;
    writeln!(file, "{}  {}", digest, path.display()).with_context(|| {
        format!(
            "Failed to write digest for '{}' into '{}'.",
            path.display(),
            output.display()
        )
    })?;
    Ok(digest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn compute_sha256_is_stable() {
        let temp = tempdir().unwrap();
        let file_path = temp.path().join("digest.bin");
        let mut file = File::create(&file_path).unwrap();
        file.write_all(b"bunker").unwrap();

        let digest = compute_sha256(&file_path).unwrap();
        assert_eq!(
            digest,
            "9078e43e365a0d2849587c33e1623ccdbd92ad1ea81c5762414e9fbee6f20c03"
        );
    }

    #[test]
    fn generate_sbom_creates_file() {
        let temp = tempdir().unwrap();
        let output = temp.path().join("bom.json");
        generate_sbom(&output).unwrap();

        let contents = std::fs::read_to_string(&output).unwrap();
        assert!(contents.contains("CycloneDX"));
        assert!(contents.contains("components"));
    }
}
