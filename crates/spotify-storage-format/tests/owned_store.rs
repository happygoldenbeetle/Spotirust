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
