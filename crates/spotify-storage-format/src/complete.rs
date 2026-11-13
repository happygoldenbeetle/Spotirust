//! Complete desktop file layout. This module owns no paths or index transactions.
use crate::{IndexRecord, StorageCipher, adler32, chunk_checksum, file_iv};
use std::io::{self, Read, Write};

pub const CHUNK_SIZE: usize = 16_384;
pub const FOOTER_SIZE: usize = 64;
const FOOTER_MAGIC: [u8; 4] = [0xab, 0x33, 0x72, 0x61];

/// A decoded storage tail, including opaque metadata and reserved footer bytes.
pub struct Trailer {
    metadata: Vec<u8>,
    checksums: Vec<u32>,
    reserved: [u8; 24],
}

pub fn stored_size(logical: u32, metadata_length: u32) -> io::Result<u32> {
    if metadata_length > 0xff_ffff {
        return Err(invalid("storage metadata too large"));
    }
    logical
        .checked_add(metadata_length)
        .and_then(|size| size.checked_add(logical.div_ceil(CHUNK_SIZE as u32) * 4))
        .and_then(|size| size.checked_add(FOOTER_SIZE as u32))
        .ok_or_else(|| invalid("storage file size overflow"))
}

impl Trailer {
    pub fn parse(record: &IndexRecord, tail: &[u8]) -> io::Result<Self> {
        if tail.len() < FOOTER_SIZE {
            return Err(invalid("missing storage footer"));
        }
        let footer = &tail[tail.len() - FOOTER_SIZE..];
        if footer[4..8] != FOOTER_MAGIC || le_u32(footer, 0) != adler32(1, &footer[4..]) {
            return Err(invalid("storage footer checksum or tag mismatch"));
        }
        if le_u32(footer, 8) != record.logical_size()
            || footer[12..32] != *record.file_id()
            || u16::from_le_bytes(footer[32..34].try_into().unwrap()) != u16::from(record.realm())
        {
            return Err(invalid("storage footer identity mismatch"));
        }
        let metadata_length = u32::from(u16::from_le_bytes(footer[34..36].try_into().unwrap()))
            | (u32::from(footer[44]) << 16);
        if stored_size(record.logical_size(), metadata_length)? != record.stored_size()
            || tail.len() as u64 + u64::from(record.logical_size())
                != u64::from(record.stored_size())
        {
            return Err(invalid("storage trailer length mismatch"));
        }
        let metadata = tail[..metadata_length as usize].to_vec();
        let expected = if metadata.is_empty() {
            u32::MAX
        } else {
            adler32(1, &metadata)
        };
        if le_u32(footer, 36) != expected {
            return Err(invalid("storage metadata checksum mismatch"));
        }
        let checksums = tail[metadata.len()..tail.len() - FOOTER_SIZE]
            .as_chunks::<4>()
            .0
            .iter()
            .map(|bytes| u32::from_be_bytes(*bytes))
            .collect();
        Ok(Self {
            metadata,
            checksums,
            reserved: footer[40..].try_into().unwrap(),
        })
    }
    pub fn metadata(&self) -> &[u8] {
        &self.metadata
    }
    pub fn reserved(&self) -> &[u8; 24] {
        &self.reserved
    }
    pub fn validate_chunk(&self, record: &IndexRecord, chunk: u32, bytes: &[u8]) -> io::Result<()> {
        let start = u64::from(chunk) * CHUNK_SIZE as u64;
        if start >= u64::from(record.logical_size()) {
            return Err(invalid("storage chunk out of range"));
        }
        let expected_length =
            (u64::from(record.logical_size()) - start).min(CHUNK_SIZE as u64) as usize;
        if bytes.len() != expected_length
            || self.checksums.get(chunk as usize).copied()
                != Some(chunk_checksum(
                    record.file_id(),
                    record.realm(),
                    chunk,
                    bytes,
                ))
        {
            return Err(invalid("storage audio chunk checksum mismatch"));
        }
        Ok(())
    }
}

/// Writes the encrypted CDN payload, metadata, per-chunk checksums, and footer.
/// Unknown footer bytes can be preserved from a parsed trailer. The caller owns
/// destination creation, publication, locking, and the corresponding index update.
/// An error can leave partial output; never pass an existing cache file as output.
pub fn write_complete(
    source: &mut impl Read,
    destination: &mut impl Write,
    record: &IndexRecord,
    cipher: &StorageCipher,
    metadata: &[u8],
    reserved: &[u8; 24],
) -> io::Result<()> {
    if record.flags() & 3 != 3 {
        return Err(invalid("writer requires a complete checksummed entry"));
    }
    let metadata_length =
        u32::try_from(metadata.len()).map_err(|_| invalid("storage metadata too large"))?;
    if stored_size(record.logical_size(), metadata_length)? != record.stored_size() {
        return Err(invalid("index does not match complete file size"));
    }
    let iv = file_iv(record.file_id(), record.realm(), record.version());
    let encrypted = record.flags() & 8 == 0;
    let mut offset = 0u64;
    let mut checksums = Vec::new();
    let mut block = [0; CHUNK_SIZE];
    while offset < u64::from(record.logical_size()) {
        let count = (u64::from(record.logical_size()) - offset).min(CHUNK_SIZE as u64) as usize;
        source.read_exact(&mut block[..count])?;
        checksums.extend_from_slice(
            &chunk_checksum(
                record.file_id(),
                record.realm(),
                (offset / CHUNK_SIZE as u64) as u32,
                &block[..count],
            )
            .to_be_bytes(),
        );
        if encrypted {
            cipher.transform_payload(&iv, offset, &mut block[..count]);
        }
        destination.write_all(&block[..count])?;
        offset += count as u64;
    }
    let mut footer = [0; FOOTER_SIZE];
    footer[4..8].copy_from_slice(&FOOTER_MAGIC);
    footer[8..12].copy_from_slice(&record.logical_size().to_le_bytes());
    footer[12..32].copy_from_slice(record.file_id());
    footer[32..34].copy_from_slice(&u16::from(record.realm()).to_le_bytes());
    footer[34..36].copy_from_slice(&(metadata_length as u16).to_le_bytes());
    let metadata_checksum = if metadata.is_empty() {
        u32::MAX
    } else {
        adler32(1, metadata)
    };
    footer[36..40].copy_from_slice(&metadata_checksum.to_le_bytes());
    footer[40..].copy_from_slice(reserved);
    footer[44] = (metadata_length >> 16) as u8;
    let checksum = adler32(1, &footer[4..]);
    footer[..4].copy_from_slice(&checksum.to_le_bytes());
    // Encrypt continuously at physical offsets, including an unaligned logical end.
    let mut tail = Vec::with_capacity(metadata.len() + checksums.len() + FOOTER_SIZE);
    tail.extend_from_slice(metadata);
    tail.extend_from_slice(&checksums);
    tail.extend_from_slice(&footer);
    if encrypted {
        cipher.transform_payload(&iv, offset, &mut tail);
    }
    destination.write_all(&tail)
}

fn le_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}
fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
