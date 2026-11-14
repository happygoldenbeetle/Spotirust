//! Experimental publication into a new store or a distinct imported copy.
use crate::{
    CURRENT_HEADER_MAGIC, INDEX_HEADER_SIZE, INDEX_RECORD_SIZE, IndexRecord, StorageCipher,
    adler32, cache_file_name, complete, reader::Catalog,
};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::Arc,
};

pub struct PartialWrite<'a> {
    pub file_id: [u8; 20],
    pub version: u16,
    pub logical_size: u32,
    pub timestamp: u32,
    pub ranges: &'a [(u64, u64)],
    pub metadata: &'a [u8],
}

pub struct WorkingStore {
    root: PathBuf,
    original_root: Option<PathBuf>,
    index: File,
    cipher: Arc<StorageCipher>,
    naming_state: [u8; 20],
    records: Vec<IndexRecord>,
    failed: bool,
}

impl WorkingStore {
    /// Opens a separate working copy while holding a read-only original handle.
    /// On Windows the exclusive working index open also rejects hard-link aliases
    /// of the original index. No source handle is opened with write access.
    pub fn open_copy(
        root: &Path,
        original_root: &Path,
        key: &[u8; 16],
        naming_state: [u8; 20],
    ) -> io::Result<Self> {
        let root = root.canonicalize()?;
        let original_root = original_root.canonicalize()?;
        if root.starts_with(&original_root) || original_root.starts_with(&root) {
            return Err(invalid(
                "working cache must be separate from original storage",
            ));
        }
        let mut original = File::open(original_root.join("Storage/index.dat"))?;
        let mut source_header = [0; INDEX_HEADER_SIZE];
        original.read_exact(&mut source_header)?;
        let index_path = root.join("Storage/index.dat").canonicalize()?;
        if !index_path.starts_with(&root) {
            return Err(invalid("working index escapes copy root"));
        }
        let mut options = OpenOptions::new();
        options.read(true).write(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            options.share_mode(0);
        }
        let mut index = options.open(index_path)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let source = original.metadata()?;
            let copy = index.metadata()?;
            if source.dev() == copy.dev() && source.ino() == copy.ino() {
                return Err(invalid("working index aliases original"));
            }
        }
        let mut header = [0; INDEX_HEADER_SIZE];
        index.read_exact(&mut header)?;
        if header != source_header || header[..16] != CURRENT_HEADER_MAGIC {
            return Err(invalid("working cache header differs from source"));
        }
        let length = index.metadata()?.len();
        if length < INDEX_HEADER_SIZE as u64
            || !(length - INDEX_HEADER_SIZE as u64).is_multiple_of(INDEX_RECORD_SIZE as u64)
        {
            return Err(invalid("invalid working index length"));
        }
        let cipher = Arc::new(StorageCipher::new(key));
        let mut records = Vec::new();
        for slot in 0..(length - INDEX_HEADER_SIZE as u64) / INDEX_RECORD_SIZE as u64 {
            let slot = u32::try_from(slot).map_err(|_| invalid("storage index too large"))?;
            let mut raw = [0; INDEX_RECORD_SIZE];
            index.read_exact(&mut raw)?;
            records.push(
                cipher
                    .decode_record(slot, &raw)
                    .map_err(|_| invalid("working index record validation failed"))?,
            );
        }
        drop(original);
        Ok(Self {
            root,
            original_root: Some(original_root),
            index,
            cipher,
            naming_state,
            records,
            failed: false,
        })
    }
    /// Creates the directory itself with create_dir, never create_dir_all. The
    /// destination must not exist. Header/key initialization remains the caller's
    /// responsibility and must be validated independently before calling.
    pub fn create_new(
        root: &Path,
        header: &[u8; INDEX_HEADER_SIZE],
        key: &[u8; 16],
        naming_state: [u8; 20],
    ) -> io::Result<Self> {
        if header[..16] != CURRENT_HEADER_MAGIC {
            return Err(invalid("unsupported storage header"));
        }
        fs::create_dir(root)?;
        let root = root.canonicalize()?;
        fs::create_dir(root.join("Data"))?;
        fs::create_dir(root.join("Storage"))?;
        let mut options = OpenOptions::new();
        options.read(true).write(true).create_new(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            options.share_mode(0);
        }
        let mut index = options.open(root.join("Storage/index.dat"))?;
        index.write_all(header)?;
        index.sync_all()?;
        Ok(Self {
            root,
            original_root: None,
            index,
            cipher: Arc::new(StorageCipher::new(key)),
            naming_state,
            records: Vec::new(),
            failed: false,
        })
    }

    /// Publishes a complete encrypted CDN stream. Does not decrypt stream audio,
    /// overwrite an existing complete entry, delete anything, or run cleanup automatically.
    /// A partial index entry can be completed by publishing a new file version.
    /// On failure, unpublished files remain for inspection and further writes stop.
    pub fn write_audio(
        &mut self,
        file_id: [u8; 20],
        version: u16,
        logical_size: u32,
        timestamp: u32,
        source: &mut (impl Read + ?Sized),
        metadata: &[u8],
    ) -> io::Result<()> {
        if self.failed {
            return Err(invalid(
                "storage writer stopped after a previous I/O failure",
            ));
        }
        let previous_slot = self
            .records
            .iter()
            .position(|r| r.realm() == 0 && r.file_id() == &file_id);
        let previous = previous_slot.map(|slot| self.records[slot].clone());
        if previous.as_ref().is_some_and(|record| record.flags() != 6) {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "audio entry already exists",
            ));
        }
        if logical_size == 0 || logical_size > i32::MAX as u32 {
            return Err(invalid("unsupported audio length"));
        }
        let inherited = if previous.is_some() && metadata.is_empty() {
            self.catalog()
                .open_partial_audio(&file_id)?
                .map(|file| file.storage_metadata().to_vec())
        } else {
            None
        };
        let metadata = inherited.as_deref().unwrap_or(metadata);
        let metadata_length =
            u32::try_from(metadata.len()).map_err(|_| invalid("storage metadata too large"))?;
        let stored_size = complete::stored_size(logical_size, metadata_length)?;
        if stored_size > i32::MAX as u32 {
            return Err(invalid("unsupported physical audio length"));
        }
        let record = new_audio_record(
            previous.as_ref(),
            file_id,
            version,
            logical_size,
            stored_size,
            timestamp,
        )?;
        // All writes target names derived from fixed-size identifiers inside this
        // newly created root. Reject junctions introduced after initialization.
        let name = cache_file_name(&self.naming_state, &file_id, 0, version);
        let data = self.root.join("Data").canonicalize()?;
        if !data.starts_with(&self.root) {
            return Err(invalid("data directory escapes owned root"));
        }
        let directory = data.join(&name[..2]);
        match fs::create_dir(&directory) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
        let directory = directory.canonicalize()?;
        if !directory.starts_with(&self.root) {
            return Err(invalid("data shard escapes owned root"));
        }
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(directory.join(name))?;
        let slot = u32::try_from(previous_slot.unwrap_or(self.records.len()))
            .map_err(|_| invalid("storage index too large"))?;
        let result = (|| {
            complete::write_complete(source, &mut file, &record, &self.cipher, metadata, &[0; 24])?;
            file.sync_all()?;
            self.index.seek(SeekFrom::Start(
                INDEX_HEADER_SIZE as u64 + u64::from(slot) * INDEX_RECORD_SIZE as u64,
            ))?;
            self.index
                .write_all(&self.cipher.encode_record(slot, &record))?;
            self.index.sync_all()
        })();
        if let Err(error) = result {
            self.failed = true;
            return Err(error);
        }
        if let Some(slot) = previous_slot {
            self.records[slot] = record;
        } else {
            self.records.push(record);
        }
        Ok(())
    }

    /// Persists whole downloaded chunks into a sparse native file. Existing
    /// partial versions stay stable, so each progress update writes only new data.
    pub fn write_partial_audio(
        &mut self,
        request: PartialWrite<'_>,
        source: &mut (impl Read + Seek + ?Sized),
    ) -> io::Result<bool> {
        let PartialWrite {
            file_id,
            version,
            logical_size,
            timestamp,
            ranges,
            metadata,
        } = request;
        if self.failed {
            return Err(invalid("storage writer stopped after an I/O failure"));
        }
        if logical_size == 0 || logical_size > i32::MAX as u32 {
            return Err(invalid("unsupported audio length"));
        }
        if !crate::partial::chunk_map(logical_size, ranges)?
            .iter()
            .any(|&value| value)
        {
            return Ok(false);
        }
        let slot = self
            .records
            .iter()
            .position(|r| r.realm() == 0 && r.file_id() == &file_id);
        let record = if let Some(slot) = slot {
            let record = self.records[slot].clone();
            if record.flags() == 7 {
                return Ok(false);
            }
            if record.flags() != 6 || record.logical_size() != logical_size {
                return Err(invalid("partial stream identity changed"));
            }
            record
        } else {
            new_audio_record(None, file_id, version, logical_size, 0, timestamp)?.with_flags(6)
        };
        let name = cache_file_name(&self.naming_state, &file_id, 0, record.version());
        let data = self.root.join("Data").canonicalize()?;
        if !data.starts_with(&self.root) {
            return Err(invalid("data directory escapes working copy"));
        }
        let directory = data.join(&name[..2]);
        match fs::create_dir(&directory) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
        let directory = directory.canonicalize()?;
        if !directory.starts_with(&self.root) {
            return Err(invalid("data shard escapes working copy"));
        }
        let path = directory.join(&name);
        if slot.is_some() && !path.canonicalize()?.starts_with(&self.root) {
            return Err(invalid("partial file escapes working copy"));
        }
        // Query original identity without requesting data or write access.
        let original_guard = if slot.is_some() {
            if let Some(original) = &self.original_root {
                match original_metadata_handle(&original.join("Data").join(&name[..2]).join(&name))
                {
                    Ok(file) => Some(file),
                    Err(e) if e.kind() == io::ErrorKind::NotFound => None,
                    Err(e) => return Err(e),
                }
            } else {
                None
            }
        } else {
            None
        };
        let mut options = OpenOptions::new();
        options.read(true).write(true).create_new(slot.is_none());
        let mut output = options.open(path)?;
        if let Some(original) = &original_guard
            && same_file(original, &output)?
        {
            return Err(invalid("partial file aliases original"));
        }
        let updated = crate::partial::update_file(
            source,
            &mut output,
            &record,
            &self.cipher,
            ranges,
            metadata,
        );
        drop(original_guard);
        let result = (|| {
            let Some(updated) = updated? else {
                return Ok(false);
            };
            let position = slot.unwrap_or(self.records.len());
            let position_u32 =
                u32::try_from(position).map_err(|_| invalid("storage index too large"))?;
            self.index.seek(SeekFrom::Start(
                INDEX_HEADER_SIZE as u64 + u64::from(position_u32) * INDEX_RECORD_SIZE as u64,
            ))?;
            self.index
                .write_all(&self.cipher.encode_record(position_u32, &updated))?;
            self.index.sync_all()?;
            if let Some(slot) = slot {
                self.records[slot] = updated;
            } else {
                self.records.push(updated);
            }
            Ok(true)
        })();
        if result.is_err() {
            self.failed = true;
        }
        result
    }

    pub fn catalog(&self) -> Catalog {
        Catalog::from_records(
            self.root.clone(),
            self.cipher.clone(),
            self.naming_state,
            &self.records,
        )
    }
}

fn new_audio_record(
    previous: Option<&IndexRecord>,
    file_id: [u8; 20],
    version: u16,
    logical: u32,
    stored: u32,
    timestamp: u32,
) -> io::Result<IndexRecord> {
    let mut raw = previous.map_or([0; INDEX_RECORD_SIZE], |record| *record.as_bytes());
    raw[4..8].copy_from_slice(&0xdeadbeef_u32.to_be_bytes());
    raw[8..28].copy_from_slice(&file_id);
    raw[30..32].copy_from_slice(&version.to_be_bytes());
    raw[32..36].copy_from_slice(&7_u32.to_be_bytes());
    raw[36..40].copy_from_slice(&logical.to_be_bytes());
    if previous.is_none() {
        raw[40..44].copy_from_slice(&timestamp.to_be_bytes());
        raw[48] = 1;
    }
    raw[44..48].copy_from_slice(&stored.to_be_bytes());
    raw[52..56].copy_from_slice(&timestamp.to_be_bytes());
    let checksum = adler32(1, &raw[4..]);
    raw[..4].copy_from_slice(&checksum.to_be_bytes());
    IndexRecord::parse(&raw).map_err(|_| invalid("invalid newly serialized record"))
}
fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn original_metadata_handle(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.access_mode(0);
    }
    #[cfg(not(windows))]
    options.read(true);
    options.open(path)
}

#[cfg(windows)]
fn same_file(a: &File, b: &File) -> io::Result<bool> {
    use std::{mem::MaybeUninit, os::windows::io::AsRawHandle};
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
    };
    let identity = |file: &File| -> io::Result<(u32, u32, u32)> {
        let mut info = MaybeUninit::<BY_HANDLE_FILE_INFORMATION>::uninit();
        if unsafe { GetFileInformationByHandle(file.as_raw_handle(), info.as_mut_ptr()) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let info = unsafe { info.assume_init() };
        Ok((
            info.dwVolumeSerialNumber,
            info.nFileIndexHigh,
            info.nFileIndexLow,
        ))
    };
    Ok(identity(a)? == identity(b)?)
}
#[cfg(unix)]
fn same_file(a: &File, b: &File) -> io::Result<bool> {
    use std::os::unix::fs::MetadataExt;
    let a = a.metadata()?;
    let b = b.metadata()?;
    Ok(a.dev() == b.dev() && a.ino() == b.ino())
}
