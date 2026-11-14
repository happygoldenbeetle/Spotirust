//! Forward recovery intent containing only new encrypted bytes and index records.
//! Never copies an old cache file or stores a source cache path.
use crate::{INDEX_HEADER_SIZE, INDEX_RECORD_SIZE, IndexRecord, StorageCipher, partial::Patch};
use sha1::{Digest, Sha1};
use std::io;

const MAGIC: &[u8; 16] = b"SPOTIRUST-TXN-01";
pub(crate) const MAX_SIZE: usize = 64 * 1024 * 1024;
pub(crate) const READY: &str = "spotirust.pending";
pub(crate) const STAGING: &str = "spotirust.pending-writing";

pub(crate) struct Transaction {
    pub header_hash: [u8; 20],
    pub index_length: u64,
    pub records: Vec<(u32, IndexRecord)>,
    pub data: Option<(IndexRecord, Vec<Patch>)>,
}

impl Transaction {
    pub fn encode(&self, cipher: &StorageCipher) -> io::Result<Vec<u8>> {
        let mut bytes = MAGIC.to_vec();
        bytes.extend_from_slice(&self.header_hash);
        bytes.extend_from_slice(&self.index_length.to_le_bytes());
        bytes.extend_from_slice(&(self.records.len() as u32).to_le_bytes());
        for (slot, record) in &self.records {
            bytes.extend_from_slice(&slot.to_le_bytes());
            bytes.extend_from_slice(&cipher.encode_record(*slot, record));
        }
        bytes.push(u8::from(self.data.is_some()));
        if let Some((record, patches)) = &self.data {
            bytes.extend_from_slice(record.as_bytes());
            bytes.extend_from_slice(&(patches.len() as u32).to_le_bytes());
            for patch in patches {
                bytes.extend_from_slice(&patch.offset.to_le_bytes());
                bytes.extend_from_slice(&(patch.bytes.len() as u32).to_le_bytes());
                bytes.extend_from_slice(&patch.bytes);
                if bytes.len() > MAX_SIZE - 20 {
                    return Err(invalid("storage transaction exceeds recovery limit"));
                }
            }
        }
        let digest = Sha1::digest(&bytes);
        bytes.extend_from_slice(&digest);
        // The same validation is used before publication and during recovery.
        Self::decode(&bytes, cipher, &self.header_hash)?;
        Ok(bytes)
    }

    pub fn decode(
        bytes: &[u8],
        cipher: &StorageCipher,
        header_hash: &[u8; 20],
    ) -> io::Result<Self> {
        if bytes.len() < 69 || bytes.len() > MAX_SIZE {
            return Err(invalid("invalid storage transaction length"));
        }
        let (body, checksum) = bytes.split_at(bytes.len() - 20);
        if Sha1::digest(body).as_slice() != checksum {
            return Err(invalid("storage transaction checksum mismatch"));
        }
        let mut cursor = Cursor(body);
        if cursor.take(16)? != MAGIC || cursor.take(20)? != header_hash {
            return Err(invalid("storage transaction belongs to a different cache"));
        }
        let index_length = cursor.u64()?;
        if index_length < INDEX_HEADER_SIZE as u64
            || !(index_length - INDEX_HEADER_SIZE as u64).is_multiple_of(INDEX_RECORD_SIZE as u64)
        {
            return Err(invalid("invalid transaction index length"));
        }
        let count = cursor.u32()? as usize;
        if count > cursor.0.len() / (4 + INDEX_RECORD_SIZE) {
            return Err(invalid("truncated transaction records"));
        }
        let mut records = Vec::with_capacity(count);
        let mut seen = std::collections::HashSet::new();
        for _ in 0..count {
            let slot = cursor.u32()?;
            if INDEX_HEADER_SIZE as u64 + (u64::from(slot) + 1) * INDEX_RECORD_SIZE as u64
                > index_length
                || !seen.insert(slot)
            {
                return Err(invalid("invalid transaction record slot"));
            }
            let record = cipher
                .decode_record(slot, cursor.take(INDEX_RECORD_SIZE)?)
                .map_err(|_| invalid("invalid transaction record"))?;
            records.push((slot, record));
        }
        let data = match cursor.take(1)?[0] {
            0 => None,
            1 => {
                let record = IndexRecord::parse(cursor.take(INDEX_RECORD_SIZE)?)
                    .map_err(|_| invalid("invalid transaction data record"))?;
                if record.realm() != 0
                    || !matches!(record.flags(), 6 | 7)
                    || record.logical_size() == 0
                    || record.logical_size() > i32::MAX as u32
                    || !records.iter().any(|(_, indexed)| indexed == &record)
                {
                    return Err(invalid("transaction data differs from indexed record"));
                }
                let count = cursor.u32()? as usize;
                if count > cursor.0.len() / 12 {
                    return Err(invalid("truncated transaction data"));
                }
                let mut patches = Vec::with_capacity(count);
                for _ in 0..count {
                    let offset = cursor.u64()?;
                    let length = cursor.u32()? as usize;
                    if offset
                        .checked_add(length as u64)
                        .is_none_or(|end| end > u64::from(record.stored_size()))
                    {
                        return Err(invalid("transaction patch exceeds data file"));
                    }
                    patches.push(Patch {
                        offset,
                        bytes: cursor.take(length)?.to_vec(),
                    });
                }
                Some((record, patches))
            }
            _ => return Err(invalid("invalid transaction data marker")),
        };
        if !cursor.0.is_empty() {
            return Err(invalid("unexpected transaction suffix"));
        }
        Ok(Self {
            header_hash: *header_hash,
            index_length,
            records,
            data,
        })
    }
}

struct Cursor<'a>(&'a [u8]);
impl<'a> Cursor<'a> {
    fn take(&mut self, count: usize) -> io::Result<&'a [u8]> {
        if count > self.0.len() {
            return Err(invalid("truncated storage transaction"));
        }
        let (head, rest) = self.0.split_at(count);
        self.0 = rest;
        Ok(head)
    }
    fn u32(&mut self) -> io::Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn u64(&mut self) -> io::Result<u64> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
}
fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
