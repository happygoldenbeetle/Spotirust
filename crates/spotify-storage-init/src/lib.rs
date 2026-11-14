//! Initializes explicit copied storage using an owned restricted native child.
//! Source cache files are only read. No audio is copied to the initializer.
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use spotify_storage_format::{
    INDEX_HEADER_SIZE, INDEX_RECORD_SIZE, StorageCipher, cache_file_name,
};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

#[cfg(windows)]
mod windows;

#[derive(Serialize, Deserialize)]
pub struct State {
    pub storage_key_hex: String,
    pub naming_state_hex: String,
    pub configured_override: u64,
}
#[derive(Serialize)]
struct Profile<'a> {
    snapshot_root: &'a Path,
    storage_key_hex: &'a str,
    naming_state_hex: &'a str,
    configured_override: u64,
    writable: bool,
}

fn original_locations() -> Result<Vec<PathBuf>> {
    let local =
        PathBuf::from(std::env::var_os("LOCALAPPDATA").context("LOCALAPPDATA unavailable")?);
    let roaming =
        PathBuf::from(std::env::var_os("APPDATA").context("APPDATA unavailable")?).join("Spotify");
    let mut locations = vec![local.join("Spotify"), roaming.clone()];
    if let Ok(prefs) = fs::read_to_string(roaming.join("prefs"))
        && let Some(value) = prefs
            .lines()
            .filter_map(|line| line.split_once('='))
            .filter(|(key, _)| key.trim() == "storage.last-location")
            .map(|(_, value)| value.trim())
            .next_back()
        && let Ok(value) = serde_json::from_str::<String>(value)
    {
        let storage = PathBuf::from(value);
        if storage.is_absolute() && storage.exists() {
            if let Some(parent) = storage
                .parent()
                .filter(|parent| parent.join("Data").is_dir())
            {
                locations.push(parent.to_path_buf());
            }
            locations.push(storage);
        }
    }
    Ok(locations
        .into_iter()
        .filter_map(|path| path.canonicalize().ok())
        .collect())
}

/// Rejects both the standard source cache and a configured external storage
/// directory. It is also used immediately before enabling working-copy writes.
pub fn validate_copy_location(snapshot: &Path) -> Result<PathBuf> {
    let snapshot = snapshot.canonicalize()?;
    for original in original_locations()? {
        anyhow::ensure!(
            !snapshot.starts_with(&original) && !original.starts_with(&snapshot),
            "working cache must be separate from original Spotify directories"
        );
    }
    Ok(snapshot)
}

pub fn initialize_copy(snapshot: &Path, output: &Path, writable: bool) -> Result<usize> {
    anyhow::ensure!(
        !output.try_exists()?,
        "existing storage profile will not be replaced"
    );
    let snapshot = validate_copy_location(snapshot)?;
    let original =
        PathBuf::from(std::env::var_os("LOCALAPPDATA").context("LOCALAPPDATA unavailable")?)
            .join("Spotify")
            .canonicalize()?;
    anyhow::ensure!(
        !snapshot.starts_with(&original) && !original.starts_with(&snapshot),
        "import must be a separate cache copy"
    );
    anyhow::ensure!(
        snapshot.join("Data").canonicalize()?.starts_with(&snapshot),
        "copied Data directory escapes import root"
    );
    anyhow::ensure!(
        snapshot
            .join("Storage/index.dat")
            .canonicalize()?
            .starts_with(&snapshot),
        "copied index escapes import root"
    );
    let mut header = [0; INDEX_HEADER_SIZE];
    fs::File::open(snapshot.join("Storage/index.dat"))?.read_exact(&mut header)?;
    anyhow::ensure!(
        header[..16] == spotify_storage_format::CURRENT_HEADER_MAGIC,
        "unsupported copied storage header"
    );
    let parent = output
        .parent()
        .context("profile must have a parent directory")?;
    let spotify = PathBuf::from(std::env::var_os("APPDATA").context("APPDATA unavailable")?)
        .join("Spotify")
        .canonicalize()?;
    let mut ancestor = parent;
    while !ancestor.exists() {
        ancestor = ancestor
            .parent()
            .context("profile directory has no existing ancestor")?;
    }
    let ancestor = ancestor.canonicalize()?;
    anyhow::ensure!(
        !ancestor.starts_with(&original) && !ancestor.starts_with(&spotify),
        "profile must be outside original Spotify directories"
    );
    anyhow::ensure!(
        !original_locations()?
            .iter()
            .any(|original| ancestor.starts_with(original)),
        "profile must be outside configured Spotify storage"
    );
    fs::create_dir_all(parent)?;
    #[cfg(windows)]
    let state = windows::capture(&snapshot, &header, parent)?;
    #[cfg(not(windows))]
    let state: State = {
        let _ = (snapshot, header, parent, writable);
        anyhow::bail!("native cache initialization requires Windows")
    };
    let key = decode::<16>(&state.storage_key_hex)?;
    let naming = decode::<20>(&state.naming_state_hex)?;
    let count = validate(&snapshot, &key, &naming)?;
    let profile = Profile {
        snapshot_root: &snapshot,
        storage_key_hex: &state.storage_key_hex,
        naming_state_hex: &state.naming_state_hex,
        configured_override: state.configured_override,
        writable,
    };
    let bytes = serde_json::to_vec_pretty(&profile)?;
    let staging = output.with_extension(format!("initializing-{}", std::process::id()));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&staging)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    drop(file);
    // Hard-link publication is atomic and refuses an existing destination.
    // Only the tiny new profile is staged; source cache files are never copied.
    fs::hard_link(&staging, output)?;
    fs::remove_file(staging)?;
    Ok(count)
}

fn validate(snapshot: &Path, key: &[u8; 16], naming: &[u8; 20]) -> Result<usize> {
    let bytes = fs::read(snapshot.join("Storage/index.dat"))?;
    anyhow::ensure!(
        bytes.len() >= INDEX_HEADER_SIZE
            && (bytes.len() - INDEX_HEADER_SIZE).is_multiple_of(INDEX_RECORD_SIZE),
        "invalid copied index length"
    );
    let cipher = StorageCipher::new(key);
    let mut count = 0;
    for (slot, bytes) in bytes[INDEX_HEADER_SIZE..]
        .as_chunks::<INDEX_RECORD_SIZE>()
        .0
        .iter()
        .enumerate()
    {
        let record = cipher
            .decode_record(u32::try_from(slot)?, bytes)
            .context("copied index validation failed")?;
        if record.stored_size() > 0 {
            let name = cache_file_name(naming, record.file_id(), record.realm(), record.version());
            let path = snapshot.join("Data").join(&name[..2]).join(name);
            anyhow::ensure!(
                path.is_file(),
                "copied cache is missing a registered data file"
            );
            anyhow::ensure!(
                path.metadata()?.len() == u64::from(record.stored_size()),
                "copied cache data length differs from index"
            );
            anyhow::ensure!(
                path.canonicalize()?.starts_with(snapshot),
                "copied data file escapes import root"
            );
        }
        if record.realm() == 0 && matches!(record.flags(), 6 | 7) {
            count += 1;
        }
    }
    Ok(count)
}

pub fn helper(bootstrap: &Path, snapshot: &Path, spotify: &Path) -> Result<()> {
    #[cfg(windows)]
    {
        windows::helper(bootstrap, snapshot, spotify)
    }
    #[cfg(not(windows))]
    {
        let _ = (bootstrap, snapshot, spotify);
        anyhow::bail!("native cache initialization requires Windows")
    }
}
fn encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn decode<const N: usize>(value: &str) -> Result<[u8; N]> {
    anyhow::ensure!(
        value.len() == N * 2 && value.is_ascii(),
        "invalid storage state length"
    );
    let mut output = [0; N];
    for (byte, pair) in output
        .iter_mut()
        .zip(value.as_bytes().as_chunks::<2>().0.iter())
    {
        *byte = u8::from_str_radix(std::str::from_utf8(pair)?, 16)?;
    }
    Ok(output)
}
fn candidate(key: &[u8; 16], index: &[u8]) -> bool {
    let cipher = StorageCipher::new(key);
    index.len() >= INDEX_HEADER_SIZE + INDEX_RECORD_SIZE
        && index[INDEX_HEADER_SIZE..]
            .as_chunks::<INDEX_RECORD_SIZE>()
            .0
            .iter()
            .take(4)
            .enumerate()
            .all(|(slot, record)| cipher.decode_record(slot as u32, record).is_ok())
}

/// Shared command routing for the desktop app and the standalone setup tool.
/// Helper dispatch happens before any GUI or authenticated player starts.
pub fn cli() -> Option<i32> {
    let arguments: Vec<_> = std::env::args_os().skip(1).collect();
    let first = arguments.first()?;
    if first == "--cache-init-helper" {
        let result = (|| -> Result<()> {
            anyhow::ensure!(arguments.len() == 4, "invalid initializer helper arguments");
            helper(
                Path::new(&arguments[1]),
                Path::new(&arguments[2]),
                Path::new(&arguments[3]),
            )
        })();
        if let Err(error) = result {
            eprintln!("Cache initializer: {error:#}");
            return Some(1);
        }
        return Some(0);
    }
    if first != "--import-spotify-cache" {
        return None;
    }
    let result = (|| -> Result<usize> {
        let snapshot = Path::new(
            arguments
                .get(1)
                .context("a copied cache directory is required")?,
        );
        let mut output = PathBuf::from(std::env::var_os("APPDATA").context("APPDATA unavailable")?)
            .join("Spotirust/storage-profile.json");
        let mut writable = false;
        let mut position = 2;
        while position < arguments.len() {
            if arguments[position] == "--writable" {
                writable = true;
            } else if arguments[position] == "--profile-output" {
                position += 1;
                output = PathBuf::from(
                    arguments
                        .get(position)
                        .context("profile output path is required")?,
                );
            } else {
                anyhow::bail!("unknown cache setup argument");
            }
            position += 1;
        }
        let output = std::path::absolute(output)?;
        initialize_copy(snapshot, &output, writable)
    })();
    match result {
        Ok(count) => {
            eprintln!("Copied audio cache initialized: {count} entries");
            Some(0)
        }
        Err(error) => {
            eprintln!("Cache setup failed: {error:#}");
            Some(1)
        }
    }
}
