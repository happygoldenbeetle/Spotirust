//! Incremental sparse native file updates. Path ownership and index publication
//! belong to the caller; this module never opens or deletes a cache path.
use crate::{
    IndexRecord, StorageCipher, chunk_checksum,
    complete::{CHUNK_SIZE, FOOTER_SIZE, Trailer, encode_tail},
    file_iv,
};
use std::{
    fs::File,
    io::{self, Read, Seek, SeekFrom, Write},
};

/// Produces whole native chunks covered by the union of downloaded byte ranges.
pub fn chunk_map(logical: u32, ranges: &[(u64, u64)]) -> io::Result<Vec<bool>> {
    if logical == 0 {
        return Err(invalid("empty partial audio"));
    }
    let mut spans = Vec::with_capacity(ranges.len());
    for &(start, length) in ranges {
        let end = start
            .checked_add(length)
            .ok_or_else(|| invalid("partial range overflow"))?;
        if end > u64::from(logical) {
            return Err(invalid("partial range exceeds audio"));
        }
        if length > 0 {
            spans.push((start, end));
        }
    }
    spans.sort_unstable();
    let mut merged: Vec<(u64, u64)> = Vec::new();
    for (start, end) in spans {
        if let Some(last) = merged.last_mut()
            && start <= last.1
        {
            last.1 = last.1.max(end);
        } else {
            merged.push((start, end));
        }
    }
    let mut chunks = vec![false; (logical as usize).div_ceil(CHUNK_SIZE)];
    for (start, end) in merged {
        let first = start.div_ceil(CHUNK_SIZE as u64) as usize;
        for (index, available) in chunks.iter_mut().enumerate().skip(first) {
            let chunk_end = ((index as u64 + 1) * CHUNK_SIZE as u64).min(u64::from(logical));
            if chunk_end > end {
                break;
            }
            *available = true;
        }
    }
    Ok(chunks)
}

/// Updates only newly completed chunks, relocating the native trailer as its
/// physical high water mark grows. The caller must serialize updates to a separate
/// owned file, never the original. A failed I/O can leave an incomplete update;
/// the caller must stop publication and handle recovery before further writes.
pub fn update_file(
    source: &mut (impl Read + Seek + ?Sized),
    destination: &mut File,
    record: &IndexRecord,
    cipher: &StorageCipher,
    ranges: &[(u64, u64)],
    metadata: &[u8],
) -> io::Result<Option<IndexRecord>> {
    let Some(update) = plan_update(source, destination, record, cipher, ranges, metadata)? else {
        return Ok(None);
    };
    for patch in &update.patches {
        destination.seek(SeekFrom::Start(patch.offset))?;
        destination.write_all(&patch.bytes)?;
    }
    destination.set_len(u64::from(update.record.stored_size()))?;
    destination.sync_all()?;
    Ok(Some(update.record))
}

pub(crate) struct Patch {
    pub offset: u64,
    pub bytes: Vec<u8>,
}
pub(crate) struct PartialUpdate {
    pub record: IndexRecord,
    pub patches: Vec<Patch>,
}

/// Builds forward writes without modifying the destination. The working store
/// can make this intent durable before it applies either data or index changes.
pub(crate) fn plan_update(
    source: &mut (impl Read + Seek + ?Sized),
    destination: &mut File,
    record: &IndexRecord,
    cipher: &StorageCipher,
    ranges: &[(u64, u64)],
    metadata: &[u8],
) -> io::Result<Option<PartialUpdate>> {
    if record.realm() != 0 || record.flags() != 6 {
        return Err(invalid(
            "incremental writer requires a partial stream entry",
        ));
    }
    let available = chunk_map(record.logical_size(), ranges)?;
    if !available.iter().any(|&value| value) {
        return Ok(None);
    }
    let iv = file_iv(record.file_id(), 0, record.version());
    let physical = destination.metadata()?.len();
    let mut checksums = vec![u32::MAX; available.len()];
    let mut data_end = 0;
    let mut inherited = None;
    let mut reserved = [0; 24];
    if physical > 0 {
        if physical != u64::from(record.stored_size()) || physical < FOOTER_SIZE as u64 {
            return Err(invalid("partial file size differs from index"));
        }
        let mut footer = [0; FOOTER_SIZE];
        destination.seek(SeekFrom::Start(physical - FOOTER_SIZE as u64))?;
        destination.read_exact(&mut footer)?;
        cipher.transform_payload(&iv, physical - FOOTER_SIZE as u64, &mut footer);
        let tail_size = Trailer::tail_size(record, &footer)?;
        data_end = physical
            .checked_sub(tail_size as u64)
            .ok_or_else(|| invalid("partial trailer exceeds file"))?;
        let mut tail = vec![0; tail_size];
        destination.seek(SeekFrom::Start(data_end))?;
        destination.read_exact(&mut tail)?;
        cipher.transform_payload(&iv, data_end, &mut tail);
        let trailer = Trailer::parse(record, &tail)?;
        checksums.copy_from_slice(trailer.checksums());
        reserved = *trailer.reserved();
        inherited = Some(trailer.metadata().to_vec());
    } else if record.stored_size() != 0 {
        return Err(invalid("indexed partial file is empty"));
    }
    let metadata = if metadata.is_empty() {
        inherited.as_deref().unwrap_or(metadata)
    } else {
        metadata
    };
    // Validate dimensions before touching any existing cached data.
    let mut checksum_bytes: Vec<u8> = checksums
        .iter()
        .flat_map(|value| value.to_be_bytes())
        .collect();
    encode_tail(record, metadata, &checksum_bytes, &reserved)?;
    let mut block = [0; CHUNK_SIZE];
    let mut patches = Vec::new();
    let mut changed = false;
    for (chunk, &ready) in available.iter().enumerate() {
        if !ready || checksums[chunk] != u32::MAX {
            continue;
        }
        let offset = chunk as u64 * CHUNK_SIZE as u64;
        let count = (u64::from(record.logical_size()) - offset).min(CHUNK_SIZE as u64) as usize;
        source.seek(SeekFrom::Start(offset))?;
        source.read_exact(&mut block[..count])?;
        checksums[chunk] = chunk_checksum(record.file_id(), 0, chunk as u32, &block[..count]);
        cipher.transform_payload(&iv, offset, &mut block[..count]);
        patches.push(Patch {
            offset,
            bytes: block[..count].to_vec(),
        });
        data_end = data_end.max(offset + count as u64);
        changed = true;
    }
    let complete = checksums.iter().all(|&value| value != u32::MAX);
    if !changed && !complete {
        return Ok(None);
    }
    if complete {
        data_end = u64::from(record.logical_size());
    }
    checksum_bytes.clear();
    checksum_bytes.extend(checksums.iter().flat_map(|value| value.to_be_bytes()));
    let mut tail = encode_tail(record, metadata, &checksum_bytes, &reserved)?;
    let stored = u32::try_from(data_end + tail.len() as u64)
        .map_err(|_| invalid("partial file too large"))?;
    cipher.transform_payload(&iv, data_end, &mut tail);
    patches.push(Patch {
        offset: data_end,
        bytes: tail,
    });
    Ok(Some(PartialUpdate {
        record: record
            .with_stored_size(stored)
            .with_flags(if complete { 7 } else { 6 }),
        patches,
    }))
}
fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
