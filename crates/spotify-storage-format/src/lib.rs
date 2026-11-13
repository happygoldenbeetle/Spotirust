//! Experimental Spotify desktop storage primitives.
//!
//! This crate does not discover cache directories, derive client keys,
//! or implement eviction. Its snapshot reader opens files read only.
//! The arithmetic and cipher primitives have been compared with Spotify Windows
//! 1.3.8.620 using synthetic data. Complete imported streams have also been
//! decoded with the ordinary authenticated playback library.

use aes::{
    Aes128,
    cipher::{BlockDecrypt, BlockEncrypt, KeyInit},
};
use sha1::{Digest, Sha1};
use std::fmt;

pub mod reader;

pub const INDEX_HEADER_SIZE: usize = 512;
pub const INDEX_RECORD_SIZE: usize = 64;
pub const CURRENT_HEADER_MAGIC: [u8; 16] = [
    0x11, 0x6f, 0x34, 0xd4, 0xa6, 0xd0, 0x35, 0x8b, 0x3c, 0xf7, 0x69, 0xa2, 0x59, 0xa9, 0xeb, 0xcc,
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FormatError {
    RecordLength,
    RecordTag,
    RecordChecksum,
}

impl fmt::Display for FormatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::RecordLength => "storage record must contain exactly 64 bytes",
            Self::RecordTag => "storage record tag is invalid",
            Self::RecordChecksum => "storage record checksum is invalid",
        })
    }
}
impl std::error::Error for FormatError {}

/// Retains unknown fields and reserved bytes when updating known fields.
#[derive(Clone, Eq, PartialEq)]
pub struct IndexRecord([u8; INDEX_RECORD_SIZE]);

impl IndexRecord {
    pub fn parse(bytes: &[u8]) -> Result<Self, FormatError> {
        let raw: [u8; INDEX_RECORD_SIZE] =
            bytes.try_into().map_err(|_| FormatError::RecordLength)?;
        if raw[4..8] != 0xdeadbeef_u32.to_be_bytes() {
            return Err(FormatError::RecordTag);
        }
        let expected = u32::from_be_bytes(raw[..4].try_into().unwrap());
        if expected != adler32(1, &raw[4..]) {
            return Err(FormatError::RecordChecksum);
        }
        Ok(Self(raw))
    }

    pub fn as_bytes(&self) -> &[u8; INDEX_RECORD_SIZE] {
        &self.0
    }
    pub fn file_id(&self) -> &[u8; 20] {
        self.0[8..28].try_into().unwrap()
    }
    pub fn realm(&self) -> u8 {
        self.0[28]
    }
    pub fn version(&self) -> u16 {
        u16::from_be_bytes(self.0[30..32].try_into().unwrap())
    }
    pub fn flags(&self) -> u32 {
        self.read_u32(32)
    }
    pub fn stored_size(&self) -> u32 {
        self.read_u32(44)
    }
    pub fn logical_size(&self) -> u32 {
        self.read_u32(36)
    }

    pub fn with_stored_size(&self, size: u32) -> Self {
        self.with_u32(44, size)
    }
    pub fn with_flags(&self, flags: u32) -> Self {
        self.with_u32(32, flags)
    }

    fn read_u32(&self, offset: usize) -> u32 {
        u32::from_be_bytes(self.0[offset..offset + 4].try_into().unwrap())
    }
    fn with_u32(&self, offset: usize, value: u32) -> Self {
        let mut raw = self.0;
        raw[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
        let checksum = adler32(1, &raw[4..]);
        raw[..4].copy_from_slice(&checksum.to_be_bytes());
        Self(raw)
    }
}

/// Receives explicit key material; it does not derive a key from an imported cache.
/// Deliberately has no Debug or serialization implementation.
pub struct StorageCipher(Aes128);

impl StorageCipher {
    pub fn new(key: &[u8; 16]) -> Self {
        Self(Aes128::new(key.into()))
    }

    pub fn encode_record(&self, slot: u32, record: &IndexRecord) -> [u8; INDEX_RECORD_SIZE] {
        let mut result = *record.as_bytes();
        let mut previous = record_iv(slot);
        for bytes in result.as_chunks_mut::<16>().0 {
            for (byte, mask) in bytes.iter_mut().zip(previous) {
                *byte ^= mask;
            }
            self.0.encrypt_block((&mut *bytes).into());
            previous.copy_from_slice(bytes);
        }
        result
    }

    pub fn decode_record(&self, slot: u32, ciphertext: &[u8]) -> Result<IndexRecord, FormatError> {
        let mut result: [u8; INDEX_RECORD_SIZE] = ciphertext
            .try_into()
            .map_err(|_| FormatError::RecordLength)?;
        let mut previous = record_iv(slot);
        for bytes in result.as_chunks_mut::<16>().0 {
            let current = *bytes;
            self.0.decrypt_block((&mut *bytes).into());
            for (byte, mask) in bytes.iter_mut().zip(previous) {
                *byte ^= mask;
            }
            previous = current;
        }
        IndexRecord::parse(&result)
    }

    /// Applies the same transform for reads and writes, at a physical storage offset.
    /// This handles bytes only, not allocation, chunk availability, or file headers.
    pub fn transform_payload(&self, iv: &[u8; 16], offset: u64, bytes: &mut [u8]) {
        let base = u128::from_be_bytes(*iv);
        let mut block_number = u128::from(offset / 16);
        let mut skip = (offset % 16) as usize;
        let mut position = 0;
        while position < bytes.len() {
            let mut stream = base.wrapping_add(block_number).to_be_bytes();
            self.0.encrypt_block((&mut stream).into());
            let count = (16 - skip).min(bytes.len() - position);
            for (byte, mask) in bytes[position..position + count]
                .iter_mut()
                .zip(&stream[skip..skip + count])
            {
                *byte ^= mask;
            }
            position += count;
            block_number = block_number.wrapping_add(1);
            skip = 0;
        }
    }
}

pub fn record_iv(slot: u32) -> [u8; 16] {
    Sha1::digest(slot.to_be_bytes())[..16].try_into().unwrap()
}

pub fn file_iv(file_id: &[u8; 20], realm: u8, version: u16) -> [u8; 16] {
    let mut iv: [u8; 16] = file_id[..16].try_into().unwrap();
    iv[0] ^= realm;
    iv[2] ^= (version >> 8) as u8;
    iv[3] ^= version as u8;
    iv
}

/// The naming state must be obtained through a separately validated initializer.
pub fn cache_file_name(state: &[u8; 20], file_id: &[u8; 20], realm: u8, version: u16) -> String {
    let mut hash = Sha1::new();
    hash.update(state);
    hash.update(file_id);
    hash.update([realm, 0]);
    hash.update(version.to_le_bytes());
    let mut name = String::with_capacity(45);
    use std::fmt::Write;
    for byte in hash.finalize() {
        write!(&mut name, "{byte:02x}").unwrap();
    }
    name.push_str(".file");
    name
}

/// Incremental Adler32 used by the observed storage helpers.
pub fn adler32(seed: u32, bytes: &[u8]) -> u32 {
    let mut a = seed & 0xffff;
    let mut b = seed >> 16;
    for chunk in bytes.chunks(5550) {
        for &byte in chunk {
            a += u32::from(byte);
            b += a;
        }
        a %= 65521;
        b %= 65521;
    }
    (b << 16) | a
}

/// The observed chunk checksum prefix excludes the per-entry version.
pub fn chunk_checksum(file_id: &[u8; 20], realm: u8, chunk: u32, bytes: &[u8]) -> u32 {
    let mut prefix = [0; 26];
    prefix[..20].copy_from_slice(file_id);
    prefix[20] = realm;
    prefix[22..].copy_from_slice(&chunk.to_be_bytes());
    adler32(adler32(1, &prefix), bytes)
}

/// Preference getter behavior, not the unresolved automatic default policy.
pub fn configured_limit_bytes(legacy_mebibytes: i32, exact_bytes: &str) -> i64 {
    if !exact_bytes.is_empty()
        && exact_bytes.bytes().all(|b| b.is_ascii_digit())
        && let Ok(value) = exact_bytes.parse::<i64>()
        && value > 0
        && value / 1_048_576 + i64::from(value % 1_048_576 != 0) == i64::from(legacy_mebibytes)
    {
        return value;
    }
    i64::from(legacy_mebibytes) * 1_048_576
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CapacityConfig {
    pub absolute_a: i64,
    pub fraction_a: f32,
    pub fraction_b: f32,
    pub absolute_b: i64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CapacityValues {
    pub first: i64,
    pub second: i64,
    pub small_disk_bound: u64,
    pub large_disk_bound: u64,
}

fn sse_truncate(value: f32) -> i64 {
    if !value.is_finite()
        || value >= 9_223_372_036_854_775_808.0
        || value < -9_223_372_036_854_775_808.0
    {
        i64::MIN
    } else {
        value as i64
    }
}

/// Reproduces the native helper's arithmetic. Names intentionally avoid assigning
/// unverified meanings to its two size thresholds. This is not an eviction policy.
pub fn capacity_values(config: CapacityConfig, disk_input: Option<u64>) -> CapacityValues {
    let (first, second, disk) = match disk_input {
        Some(disk) => {
            let size = disk as f32;
            (
                config
                    .absolute_a
                    .min(sse_truncate(size * config.fraction_a)),
                config
                    .absolute_b
                    .min(sse_truncate(size * config.fraction_b)),
                disk,
            )
        }
        None => (
            config.absolute_a,
            config.absolute_b,
            config.absolute_a as u64,
        ),
    };
    let large = disk.min(50 * 1024_u64.pow(3));
    CapacityValues {
        first,
        second,
        small_disk_bound: large.min(100 * 1024_u64.pow(2)),
        large_disk_bound: large,
    }
}
