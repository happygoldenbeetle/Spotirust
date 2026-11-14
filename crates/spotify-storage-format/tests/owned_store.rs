use spotify_storage_format::{reader::Catalog, store::WorkingStore, *};
use std::{fs, io::Read};

#[test]
fn publishes_new_native_files_and_index_without_adopting_existing_directories() {
    let parent = tempfile::tempdir().unwrap();
    let root = parent.path().join("owned");
    let mut header = [0; INDEX_HEADER_SIZE];
    header[..16].copy_from_slice(&CURRENT_HEADER_MAGIC);
    let mut store = WorkingStore::create_new(&root, &header, &[3; 16], [4; 20]).unwrap();
    assert!(WorkingStore::create_new(&root, &header, &[3; 16], [4; 20]).is_err());
    let payload = vec![0x59; 33333];
    store
        .write_audio(
            [7; 20],
            42,
            payload.len() as u32,
            1800000000,
            &mut payload.as_slice(),
            &[],
        )
        .unwrap();
    let catalog = store.catalog();
    let mut file = catalog.open_audio(&[7; 20]).unwrap().unwrap();
    let mut actual = Vec::new();
    file.read_to_end(&mut actual).unwrap();
    assert_eq!(actual, payload);
    assert!(
        store
            .write_audio([7; 20], 43, 1, 1800000000, &mut [0].as_slice(), &[])
            .is_err()
    );
    #[cfg(windows)]
    assert!(
        fs::OpenOptions::new()
            .write(true)
            .open(root.join("Storage/index.dat"))
            .is_err()
    );
    drop(store);
    let catalog = Catalog::open(&root, &[3; 16], [4; 20]).unwrap();
    assert_eq!(catalog.audio_count(), 1);
    assert_eq!(
        fs::read(root.join("Storage/index.dat")).unwrap().len(),
        INDEX_HEADER_SIZE + INDEX_RECORD_SIZE
    );
}

#[test]
fn interrupted_write_keeps_existing_data_and_does_not_publish_partial_audio() {
    let parent = tempfile::tempdir().unwrap();
    let root = parent.path().join("owned");
    let mut header = [0; INDEX_HEADER_SIZE];
    header[..16].copy_from_slice(&CURRENT_HEADER_MAGIC);
    let mut store = WorkingStore::create_new(&root, &header, &[3; 16], [4; 20]).unwrap();
    store
        .write_audio([7; 20], 42, 3, 1800000000, &mut [1, 2, 3].as_slice(), &[])
        .unwrap();
    assert!(
        store
            .write_audio([8; 20], 42, 100, 1800000000, &mut [1, 2, 3].as_slice(), &[])
            .is_err()
    );
    assert_eq!(store.catalog().audio_count(), 1);
    assert!(
        store
            .write_audio([9; 20], 42, 3, 1800000000, &mut [1, 2, 3].as_slice(), &[])
            .is_err()
    );
    drop(store);
    let catalog = Catalog::open(&root, &[3; 16], [4; 20]).unwrap();
    assert_eq!(catalog.audio_count(), 1);
    assert!(catalog.open_audio(&[8; 20]).unwrap().is_none());
}

#[test]
fn appending_to_a_distinct_copy_preserves_original_index_and_files() {
    let parent = tempfile::tempdir().unwrap();
    let original = parent.path().join("original");
    let copy = parent.path().join("copy");
    let mut header = [0; INDEX_HEADER_SIZE];
    header[..16].copy_from_slice(&CURRENT_HEADER_MAGIC);
    let mut source = WorkingStore::create_new(&original, &header, &[3; 16], [4; 20]).unwrap();
    source
        .write_audio([7; 20], 42, 3, 1800000000, &mut [1, 2, 3].as_slice(), &[])
        .unwrap();
    drop(source);
    let index = fs::read(original.join("Storage/index.dat")).unwrap();
    let name = cache_file_name(&[4; 20], &[7; 20], 0, 42);
    let relative = std::path::PathBuf::from("Data").join(&name[..2]).join(name);
    let file = fs::read(original.join(&relative)).unwrap();
    fs::create_dir_all(copy.join(relative.parent().unwrap())).unwrap();
    fs::create_dir(copy.join("Storage")).unwrap();
    fs::write(copy.join("Storage/index.dat"), &index).unwrap();
    fs::write(copy.join(&relative), &file).unwrap();
    assert!(WorkingStore::open_copy(&original, &original, &[3; 16], [4; 20]).is_err());
    let mut writer = WorkingStore::open_copy(&copy, &original, &[3; 16], [4; 20]).unwrap();
    assert_eq!(writer.catalog().audio_count(), 1);
    writer
        .write_audio([8; 20], 43, 3, 1800000001, &mut [4, 5, 6].as_slice(), &[])
        .unwrap();
    drop(writer);
    assert_eq!(
        Catalog::open(&copy, &[3; 16], [4; 20])
            .unwrap()
            .audio_count(),
        2
    );
    assert_eq!(
        Catalog::open(&original, &[3; 16], [4; 20])
            .unwrap()
            .audio_count(),
        1
    );
    assert_eq!(fs::read(original.join("Storage/index.dat")).unwrap(), index);
    assert_eq!(fs::read(original.join(relative)).unwrap(), file);
}

#[test]
fn hard_linked_original_index_is_never_opened_for_updates() {
    let parent = tempfile::tempdir().unwrap();
    let original = parent.path().join("original");
    let copy = parent.path().join("copy");
    let mut header = [0; INDEX_HEADER_SIZE];
    header[..16].copy_from_slice(&CURRENT_HEADER_MAGIC);
    drop(WorkingStore::create_new(&original, &header, &[3; 16], [4; 20]).unwrap());
    fs::create_dir_all(copy.join("Storage")).unwrap();
    fs::hard_link(
        original.join("Storage/index.dat"),
        copy.join("Storage/index.dat"),
    )
    .unwrap();
    assert!(WorkingStore::open_copy(&copy, &original, &[3; 16], [4; 20]).is_err());
    assert_eq!(
        fs::read(original.join("Storage/index.dat")).unwrap(),
        header
    );
}

#[test]
fn completes_partial_entry_in_a_new_file_while_preserving_imported_bytes() {
    let parent = tempfile::tempdir().unwrap();
    let original = parent.path().join("original");
    let copy = parent.path().join("copy");
    let control: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/partial_file.json")).unwrap();
    let hex = |s: &str| {
        s.as_bytes()
            .as_chunks::<2>()
            .0
            .iter()
            .map(|p| u8::from_str_radix(std::str::from_utf8(p).unwrap(), 16).unwrap())
            .collect::<Vec<_>>()
    };
    let key: [u8; 16] = hex(control["key"].as_str().unwrap()).try_into().unwrap();
    let record = IndexRecord::parse(&hex(control["record"].as_str().unwrap())).unwrap();
    let mut header = [0; INDEX_HEADER_SIZE];
    header[..16].copy_from_slice(&CURRENT_HEADER_MAGIC);
    let cipher = StorageCipher::new(&key);
    let mut index = header.to_vec();
    index.extend_from_slice(&cipher.encode_record(0, &record));
    let name = cache_file_name(&[7; 20], &[5; 20], 0, 3);
    let relative = std::path::PathBuf::from("Data").join(&name[..2]).join(name);
    for root in [&original, &copy] {
        fs::create_dir_all(root.join(relative.parent().unwrap())).unwrap();
        fs::create_dir(root.join("Storage")).unwrap();
        fs::write(root.join("Storage/index.dat"), &index).unwrap();
        fs::write(
            root.join(&relative),
            include_bytes!("fixtures/partial_file.bin"),
        )
        .unwrap();
    }
    let payload: Vec<_> = (0..record.logical_size() as usize)
        .map(|i| ((i * 37 + 11) % 256) as u8)
        .collect();
    let mut writer = WorkingStore::open_copy(&copy, &original, &key, [7; 20]).unwrap();
    writer
        .write_audio(
            [5; 20],
            43,
            payload.len() as u32,
            1800000000,
            &mut payload.as_slice(),
            &[],
        )
        .unwrap();
    let catalog = writer.catalog();
    assert_eq!(catalog.partial_audio_count(), 0);
    assert_eq!(catalog.audio_count(), 1);
    let mut file = catalog.open_audio(&[5; 20]).unwrap().unwrap();
    assert_eq!(file.storage_metadata(), b"\x00\x01\x00\x09synthetic");
    let mut actual = Vec::new();
    file.read_to_end(&mut actual).unwrap();
    assert_eq!(actual, payload);
    assert_eq!(
        fs::read(copy.join(&relative)).unwrap(),
        include_bytes!("fixtures/partial_file.bin")
    );
    assert_eq!(
        fs::read(original.join(&relative)).unwrap(),
        include_bytes!("fixtures/partial_file.bin")
    );
    assert_eq!(fs::read(original.join("Storage/index.dat")).unwrap(), index);
    drop(writer);
    assert_eq!(
        fs::read(copy.join("Storage/index.dat")).unwrap().len(),
        INDEX_HEADER_SIZE + INDEX_RECORD_SIZE
    );
}

#[test]
fn partial_progress_persists_across_updates_without_rewriting_known_chunks() {
    use spotify_storage_format::{complete::CHUNK_SIZE, store::PartialWrite};
    let parent = tempfile::tempdir().unwrap();
    let root = parent.path().join("owned");
    let mut header = [0; INDEX_HEADER_SIZE];
    header[..16].copy_from_slice(&CURRENT_HEADER_MAGIC);
    let mut writer = WorkingStore::create_new(&root, &header, &[3; 16], [4; 20]).unwrap();
    let payload = vec![0x59; 2 * CHUNK_SIZE + 3];
    let mut source = std::io::Cursor::new(&payload);
    let request = |version, ranges| PartialWrite {
        file_id: [7; 20],
        version,
        logical_size: payload.len() as u32,
        timestamp: 1800000000,
        ranges,
        metadata: &[],
    };
    assert!(
        writer
            .write_partial_audio(request(42, &[(0, CHUNK_SIZE as u64)]), &mut source)
            .unwrap()
    );
    let name = cache_file_name(&[4; 20], &[7; 20], 0, 42);
    let path = root.join("Data").join(&name[..2]).join(name);
    let original = fs::read(&path).unwrap();
    assert_eq!(writer.catalog().partial_audio_count(), 1);
    let mut existing_reader = writer
        .catalog()
        .open_partial_audio(&[7; 20])
        .unwrap()
        .unwrap();
    assert!(
        writer
            .write_partial_audio(
                request(111, &[(CHUNK_SIZE as u64, CHUNK_SIZE as u64 + 3)]),
                &mut source
            )
            .unwrap()
    );
    assert_eq!(
        &fs::read(&path).unwrap()[..CHUNK_SIZE],
        &original[..CHUNK_SIZE]
    );
    let mut known_chunk = vec![0; CHUNK_SIZE];
    existing_reader.read_exact(&mut known_chunk).unwrap();
    assert_eq!(known_chunk, payload[..CHUNK_SIZE]);
    let catalog = writer.catalog();
    assert_eq!(catalog.audio_count(), 1);
    assert_eq!(catalog.partial_audio_count(), 0);
    let mut file = catalog.open_audio(&[7; 20]).unwrap().unwrap();
    let mut actual = Vec::new();
    file.read_to_end(&mut actual).unwrap();
    assert_eq!(actual, payload);
    drop(writer);
    assert_eq!(
        Catalog::open(&root, &[3; 16], [4; 20])
            .unwrap()
            .audio_count(),
        1
    );
}

#[test]
fn hard_linked_partial_audio_is_rejected_before_original_bytes_change() {
    use spotify_storage_format::store::PartialWrite;
    let parent = tempfile::tempdir().unwrap();
    let original = parent.path().join("original");
    let copy = parent.path().join("copy");
    let key = [3; 16];
    let state = [4; 20];
    let mut header = [0; INDEX_HEADER_SIZE];
    header[..16].copy_from_slice(&CURRENT_HEADER_MAGIC);
    let payload = vec![0x59; 32768];
    let request = |ranges| PartialWrite {
        file_id: [7; 20],
        version: 42,
        logical_size: payload.len() as u32,
        timestamp: 1800000000,
        ranges,
        metadata: &[],
    };
    let mut source = WorkingStore::create_new(&original, &header, &key, state).unwrap();
    source
        .write_partial_audio(request(&[(0, 16384)]), &mut std::io::Cursor::new(&payload))
        .unwrap();
    drop(source);
    let name = cache_file_name(&state, &[7; 20], 0, 42);
    let relative = std::path::PathBuf::from("Data").join(&name[..2]).join(name);
    fs::create_dir_all(copy.join(relative.parent().unwrap())).unwrap();
    fs::create_dir(copy.join("Storage")).unwrap();
    let index = fs::read(original.join("Storage/index.dat")).unwrap();
    let bytes = fs::read(original.join(&relative)).unwrap();
    fs::write(copy.join("Storage/index.dat"), &index).unwrap();
    fs::hard_link(original.join(&relative), copy.join(&relative)).unwrap();
    let mut writer = WorkingStore::open_copy(&copy, &original, &key, state).unwrap();
    let error = writer
        .write_partial_audio(request(&[(0, 32768)]), &mut std::io::Cursor::new(&payload))
        .unwrap_err();
    assert!(error.to_string().contains("aliases original"));
    assert_eq!(fs::read(original.join(&relative)).unwrap(), bytes);
    assert_eq!(fs::read(original.join("Storage/index.dat")).unwrap(), index);
    drop(writer);
    assert_eq!(fs::read(copy.join("Storage/index.dat")).unwrap(), index);
}
