use spotify_storage_format::{reader::Catalog, *};
use std::{
    fs,
    io::{Read, Seek, SeekFrom},
};

fn fixture(flags: u32) -> (tempfile::TempDir, Vec<u8>) {
    let root = tempfile::tempdir().unwrap();
    let cipher = StorageCipher::new(&[9; 16]);
    let id = [5; 20];
    let payload: Vec<_> = (0..431).map(|i| (i % 251) as u8).collect();
    let mut raw = [0; 64];
    raw[4..8].copy_from_slice(&0xdeadbeef_u32.to_be_bytes());
    raw[8..28].copy_from_slice(&id);
    raw[30..32].copy_from_slice(&3_u16.to_be_bytes());
    raw[32..36].copy_from_slice(&flags.to_be_bytes());
    raw[36..40].copy_from_slice(&(payload.len() as u32).to_be_bytes());
    raw[44..48].copy_from_slice(&(payload.len() as u32 + 64).to_be_bytes());
    let checksum = adler32(1, &raw[4..]);
    raw[..4].copy_from_slice(&checksum.to_be_bytes());
    let record = IndexRecord::parse(&raw).unwrap();
    let mut index = vec![0; INDEX_HEADER_SIZE];
    index[..16].copy_from_slice(&CURRENT_HEADER_MAGIC);
    index.extend_from_slice(&cipher.encode_record(0, &record));
    fs::create_dir(root.path().join("Storage")).unwrap();
    fs::write(root.path().join("Storage/index.dat"), index).unwrap();
    let name = cache_file_name(&[7; 20], &id, 0, 3);
    let dir = root.path().join("Data").join(&name[..2]);
    fs::create_dir_all(&dir).unwrap();
    let mut bytes = payload.clone();
    bytes.extend_from_slice(&[0xaa; 64]);
    cipher.transform_payload(&file_iv(&id, 0, 3), 0, &mut bytes);
    fs::write(dir.join(name), bytes).unwrap();
    (root, payload)
}

#[test]
fn bounds_reads_to_logical_audio_and_seeks_without_exposing_tail() {
    let (root, payload) = fixture(7);
    let index_before = fs::read(root.path().join("Storage/index.dat")).unwrap();
    let catalog = Catalog::open(root.path(), &[9; 16], [7; 20]).unwrap();
    assert_eq!(catalog.audio_count(), 1);
    assert!(catalog.open_audio(&[8; 20]).unwrap().is_none());
    let mut file = catalog.open_audio(&[5; 20]).unwrap().unwrap();
    let mut actual = Vec::new();
    file.read_to_end(&mut actual).unwrap();
    assert_eq!(actual, payload);
    assert_eq!(file.seek(SeekFrom::End(-5)).unwrap(), 426);
    let mut last = [0; 20];
    assert_eq!(file.read(&mut last).unwrap(), 5);
    assert_eq!(&last[..5], &payload[426..]);
    assert!(file.seek(SeekFrom::End(-432)).is_err());
    file.seek(SeekFrom::Start(10_000)).unwrap();
    assert_eq!(file.read(&mut last).unwrap(), 0);
    file.seek(SeekFrom::Start(17)).unwrap();
    file.read_exact(&mut last).unwrap();
    assert_eq!(&last, &payload[17..37]);
    assert_eq!(
        fs::read(root.path().join("Storage/index.dat")).unwrap(),
        index_before
    );
}

#[test]
fn partial_entries_are_misses_and_wrong_keys_fail_closed() {
    let (root, _) = fixture(6);
    let catalog = Catalog::open(root.path(), &[9; 16], [7; 20]).unwrap();
    assert_eq!(catalog.audio_count(), 0);
    assert!(catalog.open_audio(&[5; 20]).unwrap().is_none());
    assert!(Catalog::open(root.path(), &[1; 16], [7; 20]).is_err());
    let path = root.path().join("Storage/index.dat");
    let mut bytes = fs::read(&path).unwrap();
    bytes.push(0);
    fs::write(&path, bytes).unwrap();
    assert!(Catalog::open(root.path(), &[9; 16], [7; 20]).is_err());
}
