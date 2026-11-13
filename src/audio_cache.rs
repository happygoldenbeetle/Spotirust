//! Explicit desktop cache snapshot integration. Writes require a separate copy.
use anyhow::{Context, Result};
use librespot_audio::cache_backend::{
    CacheBackend, CacheSource, CachedReader, PartialReader, set_cache_backend,
};
use librespot_core::FileId;
use serde::Deserialize;
use spotify_storage_format::reader::{Catalog, StorageFile};
use spotify_storage_format::store::WorkingStore;
use std::{
    collections::HashSet,
    io::{self, Read, Seek, SeekFrom},
    path::PathBuf,
    sync::{Arc, Mutex, OnceLock, RwLock},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Profile {
    snapshot_root: PathBuf,
    storage_key_hex: String,
    naming_state_hex: String,
    #[serde(default)]
    writable: bool,
}

struct ImportedCache {
    catalog: RwLock<Catalog>,
    writer: Option<Mutex<WorkingStore>>,
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
impl PartialReader for Reader {
    fn available_ranges(&self) -> Vec<(u64, u64)> {
        self.0.available_ranges()
    }
}
impl CacheBackend for ImportedCache {
    fn open(&self, file: FileId) -> io::Result<Option<Box<dyn CachedReader>>> {
        if self.rejected.lock().unwrap().contains(&file) {
            return Ok(None);
        }
        self.catalog
            .read()
            .unwrap()
            .open_audio(&file.0)
            .map(|file| file.map(|file| Box::new(Reader(file)) as Box<dyn CachedReader>))
    }
    fn reject(&self, file: FileId) {
        self.rejected.lock().unwrap().insert(file);
    }
    fn partial(&self, file: FileId) -> io::Result<Option<Box<dyn PartialReader>>> {
        if self.rejected.lock().unwrap().contains(&file) {
            return Ok(None);
        }
        self.catalog
            .read()
            .unwrap()
            .open_partial_audio(&file.0)
            .map(|file| file.map(|file| Box::new(Reader(file)) as Box<dyn PartialReader>))
    }
    fn writable(&self) -> bool {
        self.writer.is_some()
    }
    fn store(&self, file: FileId, source: &mut dyn CacheSource) -> io::Result<()> {
        let Some(writer) = &self.writer else {
            return Ok(());
        };
        let length = u32::try_from(source.seek(SeekFrom::End(0))?)
            .map_err(|_| io::Error::other("audio file exceeds native storage size"))?;
        source.seek(SeekFrom::Start(0))?;
        let timestamp = u32::try_from(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(io::Error::other)?
                .as_secs(),
        )
        .map_err(io::Error::other)?;
        let mut writer = writer.lock().unwrap();
        writer.write_audio(file.0, rand::random(), length, timestamp, source, &[])?;
        *self.catalog.write().unwrap() = writer.catalog();
        self.rejected.lock().unwrap().remove(&file);
        eprintln!("Stored completed audio in desktop cache: {length} bytes");
        Ok(())
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
    let key = decode_hex(&profile.storage_key_hex)?;
    let naming_state = decode_hex(&profile.naming_state_hex)?;
    let catalog = Catalog::open(&profile.snapshot_root, &key, naming_state)?;
    let count = catalog.audio_count();
    let writer = if profile.writable {
        let original = PathBuf::from(std::env::var_os("LOCALAPPDATA").context("LOCALAPPDATA")?)
            .join("Spotify");
        match WorkingStore::open_copy(&profile.snapshot_root, &original, &key, naming_state) {
            Ok(writer) => {
                eprintln!("Desktop audio cache writes enabled in separate working copy");
                Some(Mutex::new(writer))
            }
            Err(error) => {
                eprintln!("Desktop cache writes unavailable; retaining read-only import: {error}");
                None
            }
        }
    } else {
        None
    };
    anyhow::ensure!(
        set_cache_backend(Arc::new(ImportedCache {
            catalog: RwLock::new(catalog),
            writer,
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
            eprintln!("Imported desktop audio cache enabled: {count} complete entries")
        }
        Err(error) => eprintln!("Imported desktop audio cache unavailable: {error:#}"),
    });
}
