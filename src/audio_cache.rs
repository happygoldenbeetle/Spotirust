//! Explicitly configured, read-only desktop cache snapshot integration.
use anyhow::{Context, Result};
use librespot_audio::cache_backend::{CacheBackend, CachedReader, set_cache_backend};
use librespot_core::FileId;
use serde::Deserialize;
use spotify_storage_format::reader::{Catalog, StorageFile};
use std::{
    collections::HashSet,
    io::{self, Read, Seek, SeekFrom},
    path::PathBuf,
    sync::{Arc, Mutex, OnceLock},
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Profile {
    snapshot_root: PathBuf,
    storage_key_hex: String,
    naming_state_hex: String,
}

struct ImportedCache {
    catalog: Catalog,
    rejected: Mutex<HashSet<FileId>>,
}
struct Reader(StorageFile);
impl Read for Reader {
    fn read(&mut self, b: &mut [u8]) -> io::Result<usize> {
        self.0.read(b)
    }
}
impl Seek for Reader {
    fn seek(&mut self, p: SeekFrom) -> io::Result<u64> {
        self.0.seek(p)
    }
}
impl CachedReader for Reader {
    fn byte_len(&self) -> u64 {
        self.0.len()
    }
}
impl CacheBackend for ImportedCache {
    fn open(&self, file: FileId) -> io::Result<Option<Box<dyn CachedReader>>> {
        if self.rejected.lock().unwrap().contains(&file) {
            return Ok(None);
        }
        self.catalog
            .open_audio(&file.0)
            .map(|file| file.map(|file| Box::new(Reader(file)) as Box<dyn CachedReader>))
    }
    fn reject(&self, file: FileId) {
        self.rejected.lock().unwrap().insert(file);
    }
}

fn decode_hex<const N: usize>(value: &str) -> Result<[u8; N]> {
    anyhow::ensure!(
        value.len() == N * 2 && value.is_ascii(),
        "invalid cache profile encoding"
    );
    let mut output = [0; N];
    for (i, byte) in output.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&value[i * 2..i * 2 + 2], 16)
            .context("invalid cache profile encoding")?;
    }
    Ok(output)
}

fn install() -> Result<usize> {
    let Some(path) = std::env::var_os("SPOTIRUST_STORAGE_PROFILE") else {
        return Ok(0);
    };
    let profile: Profile =
        serde_json::from_reader(std::fs::File::open(path)?).context("invalid storage profile")?;
    let catalog = Catalog::open(
        &profile.snapshot_root,
        &decode_hex(&profile.storage_key_hex)?,
        decode_hex(&profile.naming_state_hex)?,
    )?;
    let count = catalog.audio_count();
    anyhow::ensure!(
        set_cache_backend(Arc::new(ImportedCache {
            catalog,
            rejected: Mutex::new(HashSet::new())
        })),
        "audio cache backend already installed"
    );
    Ok(count)
}

/// Disabled unless a profile is explicitly selected; errors preserve streaming.
pub fn initialize() {
    static INITIALIZED: OnceLock<()> = OnceLock::new();
    INITIALIZED.get_or_init(|| match install() {
        Ok(0) => {}
        Ok(count) => {
            eprintln!("Imported desktop audio cache enabled: {count} complete entries (read only)")
        }
        Err(error) => eprintln!("Imported desktop audio cache unavailable: {error:#}"),
    });
}
