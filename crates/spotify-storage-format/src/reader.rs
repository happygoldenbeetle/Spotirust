//! Read-only access to an explicitly selected desktop storage snapshot.
use crate::complete::{CHUNK_SIZE, FOOTER_SIZE, Trailer};
use crate::{
    CURRENT_HEADER_MAGIC, INDEX_HEADER_SIZE, INDEX_RECORD_SIZE, IndexRecord, StorageCipher,
    cache_file_name, file_iv,
};
use std::{
    collections::HashMap,
    fs::File,
    io::{self, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::Arc,
};

/// Does not discover cache paths, update timestamps, repair files, or delete data.
pub struct Catalog {
    root: PathBuf,
    cipher: Arc<StorageCipher>,
    naming_state: [u8; 20],
    audio: HashMap<[u8; 20], IndexRecord>,
}

impl Catalog {
    pub(crate) fn from_records(
        root: PathBuf,
        cipher: Arc<StorageCipher>,
        naming_state: [u8; 20],
        records: &[IndexRecord],
    ) -> Self {
        let audio = records
            .iter()
            .filter(|r| r.realm() == 0 && r.flags() == 7 && r.logical_size() > 0)
            .map(|r| (*r.file_id(), r.clone()))
            .collect();
        Self {
            root,
            cipher,
            naming_state,
            audio,
        }
    }
    pub fn open(root: &Path, key: &[u8; 16], naming_state: [u8; 20]) -> io::Result<Self> {
        let root = root.canonicalize()?;
        let mut index = File::open(root.join("Storage/index.dat"))?;
        let length = index.metadata()?.len();
        if length < INDEX_HEADER_SIZE as u64
            || !(length - INDEX_HEADER_SIZE as u64).is_multiple_of(INDEX_RECORD_SIZE as u64)
        {
            return Err(invalid("invalid storage index length"));
        }
        let mut header = [0; INDEX_HEADER_SIZE];
        index.read_exact(&mut header)?;
        if header[..16] != CURRENT_HEADER_MAGIC {
            return Err(invalid("unsupported storage index header"));
        }
        let cipher = Arc::new(StorageCipher::new(key));
        let mut audio = HashMap::new();
        for slot in 0..(length - INDEX_HEADER_SIZE as u64) / INDEX_RECORD_SIZE as u64 {
            let slot = u32::try_from(slot).map_err(|_| invalid("storage index is too large"))?;
            let mut bytes = [0; INDEX_RECORD_SIZE];
            index.read_exact(&mut bytes)?;
            let record = cipher
                .decode_record(slot, &bytes)
                .map_err(|_| invalid("storage record validation failed"))?;
            // Only the complete encrypted stream layout verified in desktop 1.3.8.620.
            // Partial files and other realms must fall back to the network for now.
            if record.realm() == 0 && record.flags() == 7 && record.logical_size() > 0 {
                audio.insert(*record.file_id(), record);
            }
        }
        Ok(Self {
            root,
            cipher,
            naming_state,
            audio,
        })
    }

    pub fn audio_count(&self) -> usize {
        self.audio.len()
    }

    pub fn open_audio(&self, file_id: &[u8; 20]) -> io::Result<Option<StorageFile>> {
        let Some(record) = self.audio.get(file_id) else {
            return Ok(None);
        };
        let name = cache_file_name(&self.naming_state, file_id, 0, record.version());
        let path = self.root.join("Data").join(&name[..2]).join(&name);
        // A snapshot must not redirect reads outside its root through junctions.
        let path = path.canonicalize()?;
        if !path.starts_with(&self.root) {
            return Err(invalid("storage entry escapes snapshot root"));
        }
        let mut file = File::open(path)?;
        let length = u64::from(record.logical_size());
        if file.metadata()?.len() != u64::from(record.stored_size())
            || u64::from(record.stored_size()) < length
        {
            return Err(invalid("storage entry length differs from index"));
        }
        let tail_length = u64::from(record.stored_size()) - length;
        let maximum_tail = 0xff_ffff
            + u64::from(record.logical_size()).div_ceil(CHUNK_SIZE as u64) * 4
            + FOOTER_SIZE as u64;
        if tail_length > maximum_tail {
            return Err(invalid("storage trailer exceeds layout bounds"));
        }
        let iv = file_iv(file_id, 0, record.version());
        let mut tail = vec![0; tail_length as usize];
        file.seek(SeekFrom::Start(length))?;
        file.read_exact(&mut tail)?;
        self.cipher.transform_payload(&iv, length, &mut tail);
        let trailer = Trailer::parse(record, &tail)?;
        Ok(Some(StorageFile {
            file,
            cipher: self.cipher.clone(),
            iv,
            position: 0,
            length,
            record: record.clone(),
            trailer,
            chunk: Vec::with_capacity(CHUNK_SIZE),
            chunk_number: None,
        }))
    }
}

pub struct StorageFile {
    file: File,
    cipher: Arc<StorageCipher>,
    iv: [u8; 16],
    position: u64,
    length: u64,
    record: IndexRecord,
    trailer: Trailer,
    chunk: Vec<u8>,
    chunk_number: Option<u32>,
}

impl StorageFile {
    pub fn len(&self) -> u64 {
        self.length
    }
    pub fn is_empty(&self) -> bool {
        self.length == 0
    }
}
impl Read for StorageFile {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        if bytes.is_empty() || self.position >= self.length {
            return Ok(0);
        }
        let chunk_number = (self.position / CHUNK_SIZE as u64) as u32;
        let chunk_start = u64::from(chunk_number) * CHUNK_SIZE as u64;
        if self.chunk_number != Some(chunk_number) {
            self.chunk_number = None;
            self.chunk.resize(
                (self.length - chunk_start).min(CHUNK_SIZE as u64) as usize,
                0,
            );
            self.file.seek(SeekFrom::Start(chunk_start))?;
            self.file.read_exact(&mut self.chunk)?;
            self.cipher
                .transform_payload(&self.iv, chunk_start, &mut self.chunk);
            self.trailer
                .validate_chunk(&self.record, chunk_number, &self.chunk)?;
            self.chunk_number = Some(chunk_number);
        }
        let within = (self.position - chunk_start) as usize;
        let count = bytes.len().min(self.chunk.len() - within);
        bytes[..count].copy_from_slice(&self.chunk[within..within + count]);
        self.position += count as u64;
        Ok(count)
    }
}
impl Seek for StorageFile {
    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        let position = match from {
            SeekFrom::Start(n) => n,
            SeekFrom::Current(n) => self
                .position
                .checked_add_signed(n)
                .ok_or_else(|| invalid("invalid seek"))?,
            SeekFrom::End(n) => self
                .length
                .checked_add_signed(n)
                .ok_or_else(|| invalid("invalid seek"))?,
        };
        self.position = position;
        Ok(position)
    }
}
fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
