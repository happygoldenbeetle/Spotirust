//! Explicit desktop cache snapshot integration. Writes require a separate copy.
use anyhow::{Context, Result};
use chrono::{Local, TimeZone};
use librespot_audio::cache_backend::{
    CacheBackend, CacheSource, CachedReader, PartialReader, set_cache_backend,
};
use librespot_core::{FileId, Session};
use librespot_playback::config::Bitrate;
use serde::Deserialize;
use spotify_storage_format::reader::{Catalog, StorageFile};
use spotify_storage_format::store::{CleanupContext, PartialWrite, WorkingStore};
use spotify_storage_format::{capacity_values, policy};
use std::{
    collections::{HashMap, HashSet},
    io::{self, Read, Seek, SeekFrom},
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
    sync::{Arc, Mutex, OnceLock, RwLock},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
static IMPORTED_ENABLED: AtomicBool = AtomicBool::new(false);
static IMPORTED: OnceLock<Arc<ImportedCache>> = OnceLock::new();
static POLICY_RUNNING: AtomicBool = AtomicBool::new(false);
type OpenAudio = Arc<Mutex<HashMap<[u8; 20], u64>>>;

#[derive(serde::Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Profile {
    snapshot_root: PathBuf,
    storage_key_hex: String,
    naming_state_hex: String,
    #[serde(default)]
    writable: bool,
    /// Captured active native setting. Zero selects automatic capacity.
    #[serde(default)]
    configured_override: u64,
    #[serde(default)]
    capacity_config: Option<spotify_storage_format::CapacityConfig>,
    #[serde(default)]
    realm_overrides: policy::RealmOverrides,
    #[serde(default)]
    source_settings_fingerprint: String,
}

struct ImportedCache {
    catalog: RwLock<Catalog>,
    writer: Option<Arc<Mutex<WorkingStore>>>,
    rejected: Mutex<HashSet<FileId>>,
    open_audio: OpenAudio,
    opened: AtomicBool,
    policy: Mutex<CapturedPolicy>,
    profile_path: PathBuf,
    snapshot_root: PathBuf,
    write_allowed: Arc<AtomicBool>,
}
#[derive(Clone)]
struct CapturedPolicy {
    configured_override: u64,
    capacity: spotify_storage_format::CapacityConfig,
    realm_overrides: policy::RealmOverrides,
    fingerprint: String,
}
struct Reader {
    file: StorageFile,
    access: Option<(FileId, Arc<Mutex<WorkingStore>>, Arc<AtomicBool>)>,
    read: bool,
    lease: Option<([u8; 20], OpenAudio)>,
}
impl Reader {
    fn new(file: StorageFile, id: FileId, cache: &ImportedCache) -> Self {
        let lease = cache.writer.as_ref().map(|_| {
            *cache.open_audio.lock().unwrap().entry(id.0).or_default() += 1;
            cache.opened.store(true, Ordering::Release);
            (id.0, cache.open_audio.clone())
        });
        Self {
            file,
            access: cache
                .writer
                .as_ref()
                .map(|writer| (id, writer.clone(), cache.write_allowed.clone())),
            read: false,
            lease,
        }
    }
}
impl Drop for Reader {
    fn drop(&mut self) {
        if self.read
            && let Some((id, writer, allowed)) = &self.access
            && allowed.load(Ordering::Acquire)
        {
            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .ok()
                .and_then(|time| u32::try_from(time.as_secs()).ok());
            if let (Some(timestamp), Ok(mut writer)) = (timestamp, writer.lock()) {
                if let Err(error) = writer.record_read(&id.0, timestamp) {
                    log::warn!("Unable to persist desktop cache access: {error}");
                }
            }
        }
        if let Some((id, open)) = &self.lease {
            let mut open = open.lock().unwrap();
            if let Some(count) = open.get_mut(id) {
                *count -= 1;
                if *count == 0 {
                    open.remove(id);
                }
            }
        }
    }
}
impl Read for Reader {
    fn read(&mut self, b: &mut [u8]) -> io::Result<usize> {
        let count = self.file.read(b)?;
        self.read |= count > 0;
        Ok(count)
    }
}
impl Seek for Reader {
    fn seek(&mut self, p: SeekFrom) -> io::Result<u64> {
        self.file.seek(p)
    }
}
impl CachedReader for Reader {
    fn byte_len(&self) -> u64 {
        self.file.len()
    }
}
impl PartialReader for Reader {
    fn available_ranges(&self) -> Vec<(u64, u64)> {
        self.file.available_ranges()
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
            .map(|reader| {
                reader.map(|reader| {
                    Box::new(Reader::new(reader, file, self)) as Box<dyn CachedReader>
                })
            })
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
            .map(|reader| {
                reader.map(|reader| {
                    Box::new(Reader::new(reader, file, self)) as Box<dyn PartialReader>
                })
            })
    }
    fn writable(&self) -> bool {
        self.writer.is_some() && self.write_allowed.load(Ordering::Acquire)
    }
    fn store_partial(
        &self,
        file: FileId,
        source: &mut dyn CacheSource,
        ranges: &[(u64, u64)],
    ) -> io::Result<()> {
        if !self.writable() {
            return Ok(());
        }
        let Some(writer) = &self.writer else {
            return Ok(());
        };
        let length = u32::try_from(source.seek(SeekFrom::End(0))?).map_err(io::Error::other)?;
        let timestamp = u32::try_from(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(io::Error::other)?
                .as_secs(),
        )
        .map_err(io::Error::other)?;
        let mut writer = writer.lock().unwrap();
        let mut catalog = self.catalog.write().unwrap();
        let request = PartialWrite {
            file_id: file.0,
            version: rand::random(),
            logical_size: length,
            timestamp,
            ranges,
            metadata: &[],
        };
        if writer.write_partial_audio(request, source)? {
            *catalog = writer.catalog();
            log::debug!("Persisted native partial audio progress");
        }
        Ok(())
    }
    fn store(&self, file: FileId, source: &mut dyn CacheSource) -> io::Result<()> {
        if !self.writable() {
            return Ok(());
        }
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
        writer.write_partial_audio(
            PartialWrite {
                file_id: file.0,
                version: rand::random(),
                logical_size: length,
                timestamp,
                ranges: &[(0, u64::from(length))],
                metadata: &[],
            },
            source,
        )?;
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
    let path = match std::env::var_os("SPOTIRUST_STORAGE_PROFILE") {
        Some(path) => PathBuf::from(path),
        None => {
            let Some(base) = std::env::var_os("APPDATA") else {
                return Ok(0);
            };
            let path = PathBuf::from(base).join("Spotirust/storage-profile.json");
            path
        }
    };
    if !path.exists() {
        let mut candidates = Vec::new();
        if let Some(root) = std::env::var_os("SPOTIRUST_CACHE_ROOT") {
            candidates.push(PathBuf::from(root));
        }
        if let Some(base) = std::env::var_os("APPDATA") {
            let base = PathBuf::from(base).join("Spotirust");
            candidates.extend([base.join("Cache"), base]);
        }
        if let Ok(executable) = std::env::current_exe() {
            if let Some(parent) = executable.parent() {
                candidates.extend([parent.join("Cache"), parent.to_path_buf()]);
            }
        }
        let root = candidates
            .into_iter()
            .find(|root| root.join("Storage/index.dat").is_file() && root.join("Data").is_dir());
        let Some(root) = root else {
            if std::env::var_os("SPOTIRUST_STORAGE_PROFILE").is_some() {
                anyhow::bail!("selected storage profile does not exist");
            }
            return Ok(0);
        };
        eprintln!("Preparing copied Spotify audio cache");
        let count = spotify_storage_init::initialize_copy(&root, &path, true)?;
        eprintln!("Copied audio cache initialized: {count} entries");
    }
    let original_profile = std::fs::read(&path)?;
    let mut profile: Profile =
        serde_json::from_slice(&original_profile).context("invalid storage profile")?;
    if profile.writable {
        let synchronize = (|| -> Result<()> {
            let stamp = spotify_storage_init::settings_fingerprint()?;
            if stamp != profile.source_settings_fingerprint {
                eprintln!("Synchronizing native storage settings for working cache");
                let state = spotify_storage_init::capture_copy_state(
                    &profile.snapshot_root,
                    path.parent().context("profile directory missing")?,
                )?;
                profile.storage_key_hex = state.storage_key_hex;
                profile.naming_state_hex = state.naming_state_hex;
                profile.configured_override = state.configured_override;
                profile.capacity_config = state.capacity_config;
                profile.realm_overrides = state.realm_overrides;
                profile.source_settings_fingerprint = state.source_settings_fingerprint;
                spotify_storage_init::replace_profile(
                    &path,
                    &original_profile,
                    &serde_json::to_vec_pretty(&profile)?,
                )?;
            }
            Ok(())
        })();
        if let Err(error) = synchronize {
            eprintln!(
                "Storage settings refresh unavailable; retaining read-only import: {error:#}"
            );
            profile.writable = false;
        }
    }
    anyhow::ensure!(
        profile.configured_override <= i64::MAX as u64,
        "unsupported native storage limit"
    );
    anyhow::ensure!(
        profile
            .capacity_config
            .is_none_or(|config| config.is_valid()),
        "invalid captured native capacity configuration"
    );
    let key = decode_hex(&profile.storage_key_hex)?;
    let naming_state = decode_hex(&profile.naming_state_hex)?;
    let writer = if profile.writable {
        let original = PathBuf::from(std::env::var_os("LOCALAPPDATA").context("LOCALAPPDATA")?)
            .join("Spotify");
        match spotify_storage_init::validate_copy_location(&profile.snapshot_root)
            .map_err(|error| io::Error::other(error.to_string()))
            .and_then(|_| {
                WorkingStore::open_copy(&profile.snapshot_root, &original, &key, naming_state)
            }) {
            Ok(writer) => {
                eprintln!("Desktop audio cache writes enabled in separate working copy");
                Some(Arc::new(Mutex::new(writer)))
            }
            Err(error) => {
                eprintln!("Desktop cache writes unavailable; retaining read-only import: {error}");
                None
            }
        }
    } else {
        None
    };
    // A pending forward transaction can repair a torn index record, so a
    // writable copy must recover before the snapshot parser sees that index.
    let catalog = match &writer {
        Some(writer) => writer.lock().unwrap().catalog(),
        None => Catalog::open(&profile.snapshot_root, &key, naming_state)?,
    };
    let count = catalog.audio_count();
    let write_allowed = Arc::new(AtomicBool::new(writer.is_some()));
    let backend = Arc::new(ImportedCache {
        catalog: RwLock::new(catalog),
        writer,
        rejected: Mutex::new(HashSet::new()),
        open_audio: Arc::new(Mutex::new(HashMap::new())),
        opened: AtomicBool::new(false),
        policy: Mutex::new(CapturedPolicy {
            configured_override: profile.configured_override,
            capacity: profile.capacity_config.unwrap_or(policy::CAPACITY_DEFAULTS),
            realm_overrides: profile.realm_overrides,
            fingerprint: profile.source_settings_fingerprint,
        }),
        profile_path: path,
        snapshot_root: profile.snapshot_root,
        write_allowed,
    });
    anyhow::ensure!(
        set_cache_backend(backend.clone()),
        "audio cache backend already installed"
    );
    let _ = IMPORTED.set(backend);
    IMPORTED_ENABLED.store(true, Ordering::Release);
    Ok(count)
}

struct DiskContext {
    free_plus_cache: u64,
    capacity: spotify_storage_format::CapacityValues,
    stored: u64,
}

impl ImportedCache {
    fn refresh_policy(&self) -> Result<CapturedPolicy> {
        spotify_storage_init::validate_copy_location(&self.snapshot_root)?;
        let original = std::fs::read(&self.profile_path)?;
        let mut profile: Profile = serde_json::from_slice(&original)?;
        anyhow::ensure!(
            profile.writable
                && profile.snapshot_root.canonicalize()? == self.snapshot_root.canonicalize()?,
            "working profile changed; restart cache integration"
        );
        let control = self
            .writer
            .as_ref()
            .context("cache is read only")?
            .lock()
            .unwrap()
            .verification_index();
        let state = spotify_storage_init::capture_control_state(
            &control,
            self.profile_path
                .parent()
                .context("profile directory missing")?,
        )?;
        anyhow::ensure!(
            state.configured_override <= i64::MAX as u64,
            "unsupported captured byte limit"
        );
        profile.storage_key_hex = state.storage_key_hex;
        profile.naming_state_hex = state.naming_state_hex;
        profile.configured_override = state.configured_override;
        profile.capacity_config = state.capacity_config;
        profile.realm_overrides = state.realm_overrides;
        profile.source_settings_fingerprint = state.source_settings_fingerprint;
        spotify_storage_init::replace_profile(
            &self.profile_path,
            &original,
            &serde_json::to_vec_pretty(&profile)?,
        )?;
        let policy = CapturedPolicy {
            configured_override: profile.configured_override,
            capacity: profile.capacity_config.unwrap_or(policy::CAPACITY_DEFAULTS),
            realm_overrides: profile.realm_overrides,
            fingerprint: profile.source_settings_fingerprint,
        };
        *self.policy.lock().unwrap() = policy.clone();
        Ok(policy)
    }
    fn policy_pass(
        &self,
        config: spotify_storage_format::CapacityConfig,
        native: &CapturedPolicy,
        orphans: bool,
    ) -> Result<(u64, DiskContext)> {
        let mut writer = self
            .writer
            .as_ref()
            .context("cache is read only")?
            .lock()
            .unwrap();
        let (free, total) = writer.disk_space()?;
        let free_plus_cache = free.saturating_add(writer.stored_bytes());
        let mut capacity = capacity_values(config, Some(total));
        if native.configured_override != 0 {
            capacity.first = native.configured_override as i64;
        }
        let now = SystemTime::now().duration_since(UNIX_EPOCH)?;
        let now_ticks = i64::try_from(now.as_secs())?
            .checked_mul(policy::TICKS_PER_SECOND)
            .and_then(|ticks| ticks.checked_add(i64::from(now.subsec_nanos() / 100)))
            .context("native cache clock overflow")?;
        let date = Local::now().date_naive();
        let midnight = Local
            .from_local_datetime(&date.and_hms_opt(0, 0, 0).unwrap())
            .earliest()
            .context("local midnight unavailable")?
            .timestamp()
            * policy::TICKS_PER_SECOND;
        let configs = policy::REALMS
            .into_iter()
            .map(|realm| (realm, native.realm_overrides.resolve(realm).unwrap()))
            .collect();
        // Opens register their lease under a catalog read lock. Holding its
        // write lock here makes the open-count snapshot and removals consistent.
        let mut catalog = self.catalog.write().unwrap();
        let open = self.open_audio.lock().unwrap().clone();
        if orphans {
            let count = writer.cleanup_orphans()?;
            if count > 0 {
                log::info!("Removed {count} orphan entries from working cache");
            }
        }
        let outcome = writer.cleanup(CleanupContext {
            configs: &configs,
            capacity,
            now_ticks,
            local_midnight_ticks: midnight,
            free_bytes: free,
            open_audio: &open,
        });
        // A failed unlink can leave the already-published index valid. Always
        // refresh the catalog, even if that final cleanup step reported an error.
        *catalog = writer.catalog();
        let outcome = outcome?;
        if !outcome.selections.is_empty() {
            eprintln!(
                "Working cache cleanup: {} entries, {} bytes reclaimed",
                outcome.selections.len(),
                outcome.reclaimed_bytes
            );
        }
        Ok((
            outcome.next_delay_ms,
            DiskContext {
                free_plus_cache,
                capacity,
                stored: writer.stored_bytes(),
            },
        ))
    }
}

/// Starts only after the player has authenticated. Imported read-only profiles
/// never start this task. Server updates come from that same account session.
pub fn start_policy_worker(session: Session) {
    let Some(cache) = IMPORTED
        .get()
        .filter(|cache| cache.writer.is_some())
        .cloned()
    else {
        return;
    };
    if POLICY_RUNNING.swap(true, Ordering::AcqRel) {
        return;
    }
    tokio::spawn(async move {
        let mut native = cache.policy.lock().unwrap().clone();
        let mut config = native.capacity;
        let mut due = Instant::now();
        let mut settings_due = Instant::now();
        let mut verified = cache.write_allowed.load(Ordering::Acquire);
        let mut disk: Option<DiskContext> = None;
        let mut first = true;
        loop {
            if session.is_invalid() {
                break;
            }
            if Instant::now() >= settings_due {
                let observed =
                    tokio::task::spawn_blocking(spotify_storage_init::settings_fingerprint).await;
                match observed {
                    Ok(Ok(stamp)) if stamp == native.fingerprint => {
                        if !verified {
                            due = Instant::now();
                        }
                        verified = true;
                        cache.write_allowed.store(true, Ordering::Release);
                    }
                    Ok(Ok(_)) => {
                        verified = false;
                        cache.write_allowed.store(false, Ordering::Release);
                        let refresh = cache.clone();
                        match tokio::task::spawn_blocking(move || refresh.refresh_policy()).await {
                            Ok(Ok(updated)) => {
                                config = updated.capacity;
                                native = updated;
                                verified = true;
                                cache.write_allowed.store(true, Ordering::Release);
                                due = Instant::now();
                                eprintln!("Working cache storage settings synchronized");
                            }
                            _ => log::warn!(
                                "Working cache writes deferred while native storage settings are unavailable"
                            ),
                        }
                    }
                    _ => {
                        verified = false;
                        cache.write_allowed.store(false, Ordering::Release);
                        log::warn!(
                            "Working cache writes deferred while source settings cannot be read"
                        );
                    }
                }
                settings_due = Instant::now() + Duration::from_secs(30);
            }
            if !verified {
                tokio::time::sleep(Duration::from_secs(1)).await;
                continue;
            }
            if let Some(updated) = policy::capacity_attribute(
                session.get_user_attribute("storage-size-config").as_deref(),
            ) {
                if updated != config {
                    config = updated;
                    due = Instant::now();
                }
            }
            if cache.opened.swap(false, Ordering::AcqRel) {
                due = due.min(Instant::now() + Duration::from_secs(600));
            }
            if let Some(disk) = &mut disk {
                let stored = cache
                    .writer
                    .as_ref()
                    .unwrap()
                    .lock()
                    .unwrap()
                    .stored_bytes();
                let growth = stored.saturating_sub(disk.stored);
                let free = disk.free_plus_cache.saturating_sub(stored);
                let reserve = disk.capacity.second.max(0) as u64;
                if free < reserve && growth > reserve - free {
                    due = due.min(Instant::now() + Duration::from_secs(300));
                }
                disk.stored = stored;
            }
            if Instant::now() >= due {
                let pass_cache = cache.clone();
                let native = native.clone();
                match tokio::task::spawn_blocking(move || {
                    pass_cache.policy_pass(config, &native, first)
                })
                .await
                {
                    Ok(Ok((delay, context))) => {
                        due = Instant::now() + Duration::from_millis(delay);
                        disk = Some(context);
                        first = false;
                    }
                    result => {
                        log::warn!(
                            "Working cache cleanup deferred after failure: {}",
                            match result {
                                Err(error) => error.to_string(),
                                Ok(Err(error)) => error.to_string(),
                                _ => unreachable!(),
                            }
                        );
                        due = Instant::now() + Duration::from_secs(300);
                    }
                }
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
        POLICY_RUNNING.store(false, Ordering::Release);
    });
}

/// Match the official account's supported streaming quality while importing its
/// cache. The preference uses Spotify's AudioQuality protocol enumeration.
pub fn preferred_bitrate(username: Option<&str>) -> Option<Bitrate> {
    if !IMPORTED_ENABLED.load(Ordering::Acquire) {
        return None;
    }
    let username = username?;
    if username.contains(['/', '\\', ':']) || username == "." || username == ".." {
        return None;
    }
    let path = PathBuf::from(std::env::var_os("APPDATA")?)
        .join("Spotify/Users")
        .join(format!("{username}-user"))
        .join("prefs");
    let prefs = std::fs::read_to_string(path).ok()?;
    let bitrate = bitrate_from_preferences(&prefs)?;
    eprintln!("Imported cache playback matches official Spotify quality: {bitrate:?}");
    Some(bitrate)
}

fn bitrate_from_preferences(prefs: &str) -> Option<Bitrate> {
    let value = |key: &str| {
        prefs
            .lines()
            .filter_map(|line| line.split_once('='))
            .filter(|(name, _)| name.trim() == key)
            .filter_map(|(_, value)| value.trim().parse::<i32>().ok())
            .next_back()
    };
    match value("audio.play_bitrate_non_metered_enumeration")
        .or_else(|| value("audio.play_bitrate_enumeration"))
    {
        Some(2) => Some(Bitrate::Bitrate96),
        Some(3) => Some(Bitrate::Bitrate160),
        Some(4) => Some(Bitrate::Bitrate320),
        _ => None,
    }
}

#[cfg(test)]
mod quality_tests {
    use super::*;
    #[test]
    fn inherits_non_metered_quality_instead_of_silently_recaching_at_160() {
        assert_eq!(
            bitrate_from_preferences(
                "audio.play_bitrate_enumeration=3\naudio.play_bitrate_non_metered_enumeration=4\n"
            ),
            Some(Bitrate::Bitrate320)
        );
        assert_eq!(
            bitrate_from_preferences("audio.play_bitrate_enumeration=2\n"),
            Some(Bitrate::Bitrate96)
        );
        assert_eq!(
            bitrate_from_preferences(
                "audio.play_bitrate_enumeration=4\naudio.play_bitrate_non_metered_enumeration=0\n"
            ),
            None
        );
    }
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
