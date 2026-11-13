use serde_json::Value;
use spotify_storage_format::{complete::CHUNK_SIZE, reader::Catalog, *};
use std::{
    fs,
    io::{Read, Seek, SeekFrom},
};

fn hex(s: &str) -> Vec<u8> {
    s.as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|p| u8::from_str_radix(std::str::from_utf8(p).unwrap(), 16).unwrap())
        .collect()
}
fn fixture() -> (tempfile::TempDir, [u8; 16], Vec<u8>) {
    let control: Value = serde_json::from_str(include_str!("fixtures/partial_file.json")).unwrap();
    let key: [u8; 16] = hex(control["key"].as_str().unwrap()).try_into().unwrap();
    let record = IndexRecord::parse(&hex(control["record"].as_str().unwrap())).unwrap();
    let cipher = StorageCipher::new(&key);
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("Storage")).unwrap();
    let mut index = vec![0; INDEX_HEADER_SIZE];
    index[..16].copy_from_slice(&CURRENT_HEADER_MAGIC);
    index.extend_from_slice(&cipher.encode_record(0, &record));
    fs::write(root.path().join("Storage/index.dat"), index).unwrap();
    let name = cache_file_name(&[7; 20], &[5; 20], 0, 3);
    let dir = root.path().join("Data").join(&name[..2]);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join(name), include_bytes!("fixtures/partial_file.bin")).unwrap();
    let payload = (0..record.logical_size() as usize)
        .map(|i| ((i * 37 + 11) % 256) as u8)
        .collect();
    (root, key, payload)
}

#[test]
fn reads_sparse_available_chunks_and_refuses_missing_audio() {
    let (root, key, payload) = fixture();
    let catalog = Catalog::open(root.path(), &key, [7; 20]).unwrap();
    assert_eq!(catalog.audio_count(), 0);
    assert_eq!(catalog.partial_audio_count(), 1);
    assert!(catalog.open_audio(&[5; 20]).unwrap().is_none());
    let mut file = catalog.open_partial_audio(&[5; 20]).unwrap().unwrap();
    assert_eq!(file.len(), payload.len() as u64);
    assert_eq!(
        file.available_ranges(),
        vec![
            (0, CHUNK_SIZE as u64),
            (2 * CHUNK_SIZE as u64, CHUNK_SIZE as u64)
        ]
    );
    let mut bytes = vec![0; CHUNK_SIZE];
    file.read_exact(&mut bytes).unwrap();
    assert_eq!(bytes, &payload[..CHUNK_SIZE]);
    assert_eq!(
        file.read(&mut [0; 1]).unwrap_err().kind(),
        std::io::ErrorKind::NotFound
    );
    file.seek(SeekFrom::Start(2 * CHUNK_SIZE as u64)).unwrap();
    file.read_exact(&mut bytes).unwrap();
    assert_eq!(bytes, &payload[2 * CHUNK_SIZE..3 * CHUNK_SIZE]);
    file.seek(SeekFrom::End(-1)).unwrap();
    assert!(file.read(&mut [0; 1]).is_err());
    file.seek(SeekFrom::End(0)).unwrap();
    assert_eq!(file.read(&mut [0; 1]).unwrap(), 0);
}

#[test]
fn rejects_a_damaged_known_chunk_without_changing_the_partial_file() {
    let (root, key, _) = fixture();
    let name = cache_file_name(&[7; 20], &[5; 20], 0, 3);
    let path = root.path().join("Data").join(&name[..2]).join(name);
    let mut bytes = fs::read(&path).unwrap();
    bytes[2 * CHUNK_SIZE + 20] ^= 1;
    fs::write(&path, &bytes).unwrap();
    let catalog = Catalog::open(root.path(), &key, [7; 20]).unwrap();
    let mut file = catalog.open_partial_audio(&[5; 20]).unwrap().unwrap();
    file.read_exact(&mut [0; 1]).unwrap();
    file.seek(SeekFrom::Start(2 * CHUNK_SIZE as u64)).unwrap();
    assert_eq!(
        file.read(&mut [0; 1]).unwrap_err().kind(),
        std::io::ErrorKind::InvalidData
    );
    assert_eq!(fs::read(path).unwrap(), bytes);
}
