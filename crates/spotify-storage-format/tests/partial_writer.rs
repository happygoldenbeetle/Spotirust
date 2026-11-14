use serde_json::Value;
use spotify_storage_format::{
    complete::{CHUNK_SIZE, Trailer},
    partial::{chunk_map, update_file},
    *,
};
use std::io::{Cursor, Read, Seek, SeekFrom};

fn hex(s: &str) -> Vec<u8> {
    s.as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|p| u8::from_str_radix(std::str::from_utf8(p).unwrap(), 16).unwrap())
        .collect()
}
#[test]
fn sparse_updates_preserve_native_chunks_and_relocate_the_trailer() {
    let control: Value = serde_json::from_str(include_str!("fixtures/partial_file.json")).unwrap();
    let key = hex(control["key"].as_str().unwrap()).try_into().unwrap();
    let cipher = StorageCipher::new(&key);
    let template = IndexRecord::parse(&hex(control["record"].as_str().unwrap())).unwrap();
    let payload: Vec<_> = (0..template.logical_size() as usize)
        .map(|i| ((i * 37 + 11) % 256) as u8)
        .collect();
    let mut source = Cursor::new(&payload);
    let mut output = tempfile::tempfile().unwrap();
    let size = CHUNK_SIZE as u64;
    let mut record = update_file(
        &mut source,
        &mut output,
        &template.with_stored_size(0),
        &cipher,
        &[(0, size), (2 * size, size)],
        b"\x00\x01\x00\x09synthetic",
    )
    .unwrap()
    .unwrap();
    assert_eq!(record.stored_size(), template.stored_size());
    assert_eq!(record.flags(), 6);
    output.rewind().unwrap();
    let mut bytes = Vec::new();
    output.read_to_end(&mut bytes).unwrap();
    let golden = include_bytes!("fixtures/partial_file.bin");
    assert_eq!(&bytes[..CHUNK_SIZE], &golden[..CHUNK_SIZE]);
    assert_eq!(&bytes[2 * CHUNK_SIZE..], &golden[2 * CHUNK_SIZE..]);
    let original_first = bytes[..CHUNK_SIZE].to_vec();
    // Fill an interior hole without moving the high water mark.
    record = update_file(
        &mut source,
        &mut output,
        &record,
        &cipher,
        &[(size, size)],
        &[],
    )
    .unwrap()
    .unwrap();
    assert_eq!(record.stored_size(), template.stored_size());
    // Then cache a short last chunk, moving the trailer across a missing chunk.
    record = update_file(
        &mut source,
        &mut output,
        &record,
        &cipher,
        &[(4 * size, 431)],
        &[],
    )
    .unwrap()
    .unwrap();
    assert_eq!(record.flags(), 6);
    output.rewind().unwrap();
    bytes.clear();
    output.read_to_end(&mut bytes).unwrap();
    assert_eq!(&bytes[..CHUNK_SIZE], original_first);
    let mut plain = bytes.clone();
    cipher.transform_payload(
        &file_iv(record.file_id(), 0, record.version()),
        0,
        &mut plain,
    );
    let tail_size = Trailer::tail_size(&record, &plain[plain.len() - 64..]).unwrap();
    let trailer = Trailer::parse(&record, &plain[plain.len() - tail_size..]).unwrap();
    assert!(!trailer.chunk_available(3));
    assert!(trailer.chunk_available(4));
    // Filling the final hole produces a complete native file with the same version.
    record = update_file(
        &mut source,
        &mut output,
        &record,
        &cipher,
        &[(3 * size, size)],
        &[],
    )
    .unwrap()
    .unwrap();
    assert_eq!(record.flags(), 7);
    assert_eq!(record.version(), 3);
    output.rewind().unwrap();
    bytes.clear();
    output.read_to_end(&mut bytes).unwrap();
    cipher.transform_payload(&file_iv(record.file_id(), 0, 3), 0, &mut bytes);
    assert_eq!(&bytes[..payload.len()], payload);
    let trailer = Trailer::parse(&record, &bytes[payload.len()..]).unwrap();
    for (i, chunk) in payload.chunks(CHUNK_SIZE).enumerate() {
        trailer.validate_chunk(&record, i as u32, chunk).unwrap();
    }
}

#[test]
fn only_whole_downloaded_chunks_are_committed_and_bad_ranges_do_not_touch_output() {
    let size = CHUNK_SIZE as u64;
    assert_eq!(
        chunk_map(
            2 * CHUNK_SIZE as u32 + 3,
            &[(0, 8), (8, size - 8), (2 * size, 3)]
        )
        .unwrap(),
        vec![true, false, true]
    );
    assert_eq!(
        chunk_map(2 * CHUNK_SIZE as u32, &[(1, size - 1), (size, 5)]).unwrap(),
        vec![false, false]
    );
    assert!(chunk_map(20, &[(u64::MAX, 2)]).is_err());
    let control: Value = serde_json::from_str(include_str!("fixtures/partial_file.json")).unwrap();
    let key = hex(control["key"].as_str().unwrap()).try_into().unwrap();
    let cipher = StorageCipher::new(&key);
    let record = IndexRecord::parse(&hex(control["record"].as_str().unwrap()))
        .unwrap()
        .with_stored_size(0);
    let mut output = tempfile::tempfile().unwrap();
    assert!(
        update_file(
            &mut Cursor::new(vec![0; 100]),
            &mut output,
            &record,
            &cipher,
            &[(u64::MAX, 2)],
            &[]
        )
        .is_err()
    );
    assert_eq!(output.metadata().unwrap().len(), 0);
    assert_eq!(output.seek(SeekFrom::End(0)).unwrap(), 0);
}
