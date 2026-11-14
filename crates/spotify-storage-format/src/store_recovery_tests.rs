use super::*;
use std::io::Cursor;

const KEY: [u8; 16] = [3; 16];
const STATE: [u8; 20] = [4; 20];

fn setup(parent: &Path) -> (PathBuf, PathBuf, WorkingStore, Vec<u8>) {
    let original = parent.join("original");
    let copy = parent.join("copy");
    let mut header = [0; INDEX_HEADER_SIZE];
    header[..16].copy_from_slice(&CURRENT_HEADER_MAGIC);
    drop(WorkingStore::create_new(&original, &header, &KEY, STATE).unwrap());
    let mut writer = WorkingStore::create_new(&copy, &header, &KEY, STATE).unwrap();
    let payload: Vec<u8> = (0..32768).map(|i| (i % 251) as u8).collect();
    writer
        .write_partial_audio(
            PartialWrite {
                file_id: [7; 20],
                version: 42,
                logical_size: payload.len() as u32,
                timestamp: 1800000000,
                ranges: &[(0, 16384)],
                metadata: &[],
            },
            &mut Cursor::new(&payload),
        )
        .unwrap();
    (original, copy, writer, payload)
}

fn finish_intent(writer: &WorkingStore, payload: &[u8]) -> Transaction {
    let record = &writer.records[0];
    let mut output = writer.open_data(record, false, false).unwrap();
    let planned = crate::partial::plan_update(
        &mut Cursor::new(payload),
        &mut output,
        record,
        &writer.cipher,
        &[(0, payload.len() as u64)],
        &[],
    )
    .unwrap()
    .unwrap();
    Transaction {
        header_hash: writer.header_hash,
        index_length: (INDEX_HEADER_SIZE + INDEX_RECORD_SIZE) as u64,
        records: vec![(0, planned.record.clone())],
        data: Some((planned.record, planned.patches)),
    }
}

fn publish_intent(writer: &WorkingStore, transaction: &Transaction) -> Vec<u8> {
    let bytes = transaction.encode(&writer.cipher).unwrap();
    let (_, ready) = writer.journal_paths().unwrap();
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(ready)
        .unwrap();
    file.write_all(&bytes).unwrap();
    file.sync_all().unwrap();
    bytes
}

#[test]
fn full_range_completion_keeps_partial_filename_and_existing_encrypted_chunks() {
    let parent = tempfile::tempdir().unwrap();
    let (original, copy, mut writer, payload) = setup(parent.path());
    let original_index = fs::read(original.join("Storage/index.dat")).unwrap();
    let name = cache_file_name(&STATE, &[7; 20], 0, 42);
    let path = copy.join("Data").join(&name[..2]).join(&name);
    let prefix = fs::read(&path).unwrap()[..16384].to_vec();
    assert!(
        writer
            .write_partial_audio(
                PartialWrite {
                    file_id: [7; 20],
                    version: 99,
                    logical_size: payload.len() as u32,
                    timestamp: 1800000100,
                    ranges: &[(0, payload.len() as u64)],
                    metadata: &[],
                },
                &mut Cursor::new(&payload),
            )
            .unwrap()
    );
    assert_eq!(writer.records.len(), 1);
    assert_eq!(writer.records[0].version(), 42);
    assert_eq!(writer.records[0].flags(), 7);
    assert_eq!(&fs::read(&path).unwrap()[..16384], prefix);
    assert_eq!(fs::read_dir(path.parent().unwrap()).unwrap().count(), 1);
    let mut decoded = Vec::new();
    writer
        .catalog()
        .open_audio(&[7; 20])
        .unwrap()
        .unwrap()
        .read_to_end(&mut decoded)
        .unwrap();
    assert_eq!(decoded, payload);
    assert_eq!(
        fs::read(original.join("Storage/index.dat")).unwrap(),
        original_index
    );
}

#[test]
fn forward_recovery_repairs_every_interruption_boundary_without_an_old_file_copy() {
    for boundary in 0..5 {
        let parent = tempfile::tempdir().unwrap();
        let (original, copy, mut writer, payload) = setup(parent.path());
        let original_index = fs::read(original.join("Storage/index.dat")).unwrap();
        let transaction = finish_intent(&writer, &payload);
        publish_intent(&writer, &transaction);
        let (record, patches) = transaction.data.as_ref().unwrap();
        let mut output = writer.open_data(record, false, false).unwrap();
        if boundary >= 1 {
            output.seek(SeekFrom::Start(patches[0].offset)).unwrap();
            output.write_all(&patches[0].bytes[..8192]).unwrap();
            output.sync_all().unwrap();
        }
        if boundary >= 2 {
            for patch in patches {
                output.seek(SeekFrom::Start(patch.offset)).unwrap();
                output.write_all(&patch.bytes).unwrap();
            }
            output.set_len(u64::from(record.stored_size())).unwrap();
            output.sync_all().unwrap();
        }
        if boundary >= 3 {
            let encoded = writer.cipher.encode_record(0, record);
            writer
                .index
                .seek(SeekFrom::Start(INDEX_HEADER_SIZE as u64))
                .unwrap();
            writer.index.write_all(&encoded[..31]).unwrap();
            writer.index.sync_all().unwrap();
        }
        if boundary >= 4 {
            writer.apply_transaction(&transaction).unwrap();
        }
        drop(output);
        drop(writer);
        let writer = WorkingStore::open_copy(&copy, &original, &KEY, STATE).unwrap();
        let mut reader = writer.catalog().open_audio(&[7; 20]).unwrap().unwrap();
        let mut actual = Vec::new();
        reader.read_to_end(&mut actual).unwrap();
        assert_eq!(actual, payload, "boundary {boundary}");
        assert!(!copy.join("Storage").join(journal::READY).exists());
        assert_eq!(
            fs::read(original.join("Storage/index.dat")).unwrap(),
            original_index
        );
        drop(writer);
        assert_eq!(
            WorkingStore::open_copy(&copy, &original, &KEY, STATE)
                .unwrap()
                .catalog()
                .audio_count(),
            1
        );
    }
}

#[test]
fn invalid_ready_intent_never_modifies_data_or_index() {
    let parent = tempfile::tempdir().unwrap();
    let (original, copy, writer, payload) = setup(parent.path());
    let transaction = finish_intent(&writer, &payload);
    let bytes = transaction.encode(&writer.cipher).unwrap();
    let (_, ready) = writer.journal_paths().unwrap();
    let record = &writer.records[0];
    let name = cache_file_name(&STATE, record.file_id(), 0, record.version());
    let path = copy.join("Data").join(&name[..2]).join(name);
    let data = fs::read(&path).unwrap();
    drop(writer);
    let index = fs::read(copy.join("Storage/index.dat")).unwrap();
    for cut in [1, 20, bytes.len() - 1] {
        fs::write(&ready, &bytes[..cut]).unwrap();
        assert!(WorkingStore::open_copy(&copy, &original, &KEY, STATE).is_err());
        assert_eq!(fs::read(copy.join("Storage/index.dat")).unwrap(), index);
        assert_eq!(fs::read(&path).unwrap(), data);
    }
}

#[test]
fn uncommitted_intent_is_discarded_without_touching_native_files() {
    let parent = tempfile::tempdir().unwrap();
    let (original, copy, writer, _) = setup(parent.path());
    let (staging, _) = writer.journal_paths().unwrap();
    fs::write(&staging, b"an interrupted intent write").unwrap();
    drop(writer);
    let index = fs::read(copy.join("Storage/index.dat")).unwrap();
    let writer = WorkingStore::open_copy(&copy, &original, &KEY, STATE).unwrap();
    assert!(!staging.exists());
    assert_eq!(writer.catalog().partial_audio_count(), 1);
    drop(writer);
    assert_eq!(fs::read(copy.join("Storage/index.dat")).unwrap(), index);
}

#[test]
fn recovery_rejects_original_audio_alias_before_replaying_any_bytes() {
    let parent = tempfile::tempdir().unwrap();
    let (original, copy, writer, payload) = setup(parent.path());
    let transaction = finish_intent(&writer, &payload);
    publish_intent(&writer, &transaction);
    let record = &writer.records[0];
    let name = cache_file_name(&STATE, record.file_id(), 0, record.version());
    let relative = PathBuf::from("Data").join(&name[..2]).join(name);
    fs::create_dir_all(original.join(relative.parent().unwrap())).unwrap();
    fs::hard_link(copy.join(&relative), original.join(&relative)).unwrap();
    let data = fs::read(original.join(&relative)).unwrap();
    drop(writer);
    let index = fs::read(copy.join("Storage/index.dat")).unwrap();
    assert!(WorkingStore::open_copy(&copy, &original, &KEY, STATE).is_err());
    assert_eq!(fs::read(original.join(relative)).unwrap(), data);
    assert_eq!(fs::read(copy.join("Storage/index.dat")).unwrap(), index);
}

#[test]
fn recovery_finishes_a_torn_appended_index_record() {
    let parent = tempfile::tempdir().unwrap();
    let (original, copy, mut writer, _) = setup(parent.path());
    writer
        .write_audio([8; 20], 19, 3, 1800000000, &mut [1, 2, 3].as_slice(), &[])
        .unwrap();
    let record = writer.records[1].clone();
    let transaction = Transaction {
        header_hash: writer.header_hash,
        index_length: (INDEX_HEADER_SIZE + 2 * INDEX_RECORD_SIZE) as u64,
        records: vec![(1, record)],
        data: None,
    };
    publish_intent(&writer, &transaction);
    writer
        .index
        .set_len((INDEX_HEADER_SIZE + INDEX_RECORD_SIZE + 17) as u64)
        .unwrap();
    writer.index.sync_all().unwrap();
    drop(writer);
    let writer = WorkingStore::open_copy(&copy, &original, &KEY, STATE).unwrap();
    let mut reader = writer.catalog().open_audio(&[8; 20]).unwrap().unwrap();
    let mut actual = Vec::new();
    reader.read_to_end(&mut actual).unwrap();
    assert_eq!(actual, [1, 2, 3]);
}

#[test]
fn read_close_updates_only_working_index_and_survives_reopen() {
    let parent = tempfile::tempdir().unwrap();
    let (original, copy, mut writer, _) = setup(parent.path());
    let before = writer.records[0].clone();
    let name = cache_file_name(&STATE, before.file_id(), 0, before.version());
    let path = copy.join("Data").join(&name[..2]).join(name);
    let bytes = fs::read(&path).unwrap();
    let original_index = fs::read(original.join("Storage/index.dat")).unwrap();
    assert!(writer.record_read(&[7; 20], 1800001234).unwrap());
    assert!(!writer.record_read(&[8; 20], 1800001234).unwrap());
    assert!(!writer.record_read(&[7; 20], 0).unwrap());
    drop(writer);
    let writer = WorkingStore::open_copy(&copy, &original, &KEY, STATE).unwrap();
    assert_eq!(writer.records[0].last_used(), 1800001234);
    assert_eq!(writer.records[0].access_count(), before.access_count() + 1);
    assert_eq!(writer.records[0].created(), before.created());
    assert_eq!(fs::read(path).unwrap(), bytes);
    assert_eq!(
        fs::read(original.join("Storage/index.dat")).unwrap(),
        original_index
    );
}

#[test]
fn capacity_cleanup_preserves_open_audio_and_reencodes_swapped_slots() {
    let parent = tempfile::tempdir().unwrap();
    let (original, copy, mut writer, _) = setup(parent.path());
    let now = 1800100000_i64;
    writer
        .write_audio(
            [8; 20],
            19,
            3,
            (now - 7200) as u32,
            &mut [1, 2, 3].as_slice(),
            &[],
        )
        .unwrap();
    writer
        .write_audio(
            [9; 20],
            20,
            3,
            (now - 4000) as u32,
            &mut [4, 5, 6].as_slice(),
            &[],
        )
        .unwrap();
    let keep = writer.records[2].stored_size();
    let configs = crate::policy::REALMS
        .into_iter()
        .map(|realm| (realm, crate::policy::default_realm(realm).unwrap()))
        .collect();
    let open = HashMap::from([([9; 20], 1)]);
    let result = writer
        .cleanup(CleanupContext {
            configs: &configs,
            capacity: crate::CapacityValues {
                first: keep as i64,
                second: 0,
                small_disk_bound: 0,
                large_disk_bound: 0,
            },
            now_ticks: now * crate::policy::TICKS_PER_SECOND,
            local_midnight_ticks: (now / 86400 * 86400) * crate::policy::TICKS_PER_SECOND,
            free_bytes: 1_000_000_000,
            open_audio: &open,
        })
        .unwrap();
    assert_eq!(result.selections.len(), 2);
    assert_eq!(writer.records.len(), 1);
    assert_eq!(writer.records[0].file_id(), &[9; 20]);
    assert_eq!(writer.cleanup_orphans().unwrap(), 0);
    drop(writer);
    let writer = WorkingStore::open_copy(&copy, &original, &KEY, STATE).unwrap();
    let mut audio = writer.catalog().open_audio(&[9; 20]).unwrap().unwrap();
    let mut bytes = Vec::new();
    audio.read_to_end(&mut bytes).unwrap();
    assert_eq!(bytes, [4, 5, 6]);
    assert_eq!(writer.records.len(), 1);
}

#[test]
fn expiry_and_orphan_cleanup_preserve_registered_data_and_original_files() {
    let parent = tempfile::tempdir().unwrap();
    let (original, copy, mut writer, _) = setup(parent.path());
    // A source data file with the same native name remains entirely distinct.
    let record = writer.records[0].clone();
    let name = cache_file_name(&STATE, record.file_id(), 0, record.version());
    let relative = PathBuf::from("Data").join(&name[..2]).join(name);
    fs::create_dir_all(original.join(relative.parent().unwrap())).unwrap();
    fs::write(original.join(&relative), b"original must remain").unwrap();
    let now = 1800000000_i64 + 80 * 86400;
    writer
        .write_audio([8; 20], 19, 3, now as u32, &mut [1, 2, 3].as_slice(), &[])
        .unwrap();
    drop(writer);
    let mut writer = WorkingStore::open_copy(&copy, &original, &KEY, STATE).unwrap();
    let configs = crate::policy::REALMS
        .into_iter()
        .map(|realm| (realm, crate::policy::default_realm(realm).unwrap()))
        .collect();
    let result = writer
        .cleanup(CleanupContext {
            configs: &configs,
            capacity: crate::CapacityValues {
                first: 1_000_000_000,
                second: 0,
                small_disk_bound: 0,
                large_disk_bound: 0,
            },
            now_ticks: now * crate::policy::TICKS_PER_SECOND,
            local_midnight_ticks: (now / 86400 * 86400) * crate::policy::TICKS_PER_SECOND,
            free_bytes: 1_000_000_000,
            open_audio: &HashMap::new(),
        })
        .unwrap();
    assert_eq!(result.selections.len(), 1);
    assert_eq!(
        result.selections[0].reason,
        crate::policy::Reason::ExpiredUsed
    );
    assert!(!copy.join(&relative).exists());
    assert_eq!(
        fs::read(original.join(&relative)).unwrap(),
        b"original must remain"
    );
    let orphan_name = format!("{}.file", "a".repeat(40));
    fs::create_dir(copy.join("Data/aa")).unwrap();
    fs::write(
        copy.join("Data/aa").join(&orphan_name),
        b"unregistered native file",
    )
    .unwrap();
    fs::write(copy.join("Data/aa/user-note.txt"), b"retain unrelated file").unwrap();
    assert_eq!(writer.cleanup_orphans().unwrap(), 1);
    assert!(copy.join("Data/aa/user-note.txt").exists());
    assert_eq!(writer.catalog().audio_count(), 1);
}

#[test]
fn cleanup_rejects_an_original_alias_before_removing_an_index_entry() {
    let parent = tempfile::tempdir().unwrap();
    let (original, copy, writer, _) = setup(parent.path());
    let record = writer.records[0].clone();
    let name = cache_file_name(&STATE, record.file_id(), 0, record.version());
    let relative = PathBuf::from("Data").join(&name[..2]).join(name);
    fs::create_dir_all(original.join(relative.parent().unwrap())).unwrap();
    fs::hard_link(copy.join(&relative), original.join(&relative)).unwrap();
    let bytes = fs::read(original.join(&relative)).unwrap();
    drop(writer);
    let index = fs::read(copy.join("Storage/index.dat")).unwrap();
    let mut writer = WorkingStore::open_copy(&copy, &original, &KEY, STATE).unwrap();
    let configs = crate::policy::REALMS
        .into_iter()
        .map(|realm| (realm, crate::policy::default_realm(realm).unwrap()))
        .collect();
    let now = 1800000000_i64 + 80 * 86400;
    assert!(
        writer
            .cleanup(CleanupContext {
                configs: &configs,
                capacity: crate::CapacityValues {
                    first: 1_000_000_000,
                    second: 0,
                    small_disk_bound: 0,
                    large_disk_bound: 0
                },
                now_ticks: now * crate::policy::TICKS_PER_SECOND,
                local_midnight_ticks: (now / 86400 * 86400) * crate::policy::TICKS_PER_SECOND,
                free_bytes: 1_000_000_000,
                open_audio: &HashMap::new()
            })
            .is_err()
    );
    assert_eq!(writer.catalog().partial_audio_count(), 1);
    drop(writer);
    assert_eq!(fs::read(copy.join("Storage/index.dat")).unwrap(), index);
    assert_eq!(fs::read(original.join(&relative)).unwrap(), bytes);
    assert!(copy.join(relative).exists());
}

#[test]
fn verification_control_does_not_open_or_modify_any_registered_audio() {
    let parent = tempfile::tempdir().unwrap();
    let (original, copy, writer, _) = setup(parent.path());
    let control = writer.verification_index();
    assert_eq!(control.len(), INDEX_HEADER_SIZE + INDEX_RECORD_SIZE);
    assert_eq!(
        StorageCipher::new(&KEY)
            .decode_record(0, &control[INDEX_HEADER_SIZE..])
            .unwrap()
            .logical_size(),
        1
    );
    let before = writer.records[0].clone();
    drop(writer);
    let writer = WorkingStore::open_copy(&copy, &original, &KEY, STATE).unwrap();
    assert_eq!(writer.records[0].as_bytes(), before.as_bytes());
    assert_eq!(writer.verification_index(), control);
}
