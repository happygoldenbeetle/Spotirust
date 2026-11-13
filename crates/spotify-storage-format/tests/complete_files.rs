use serde_json::Value;
use sha1::{Digest, Sha1};
use spotify_storage_format::{complete::*, *};
use std::io::{self, Cursor};

fn hex(s: &str) -> Vec<u8> {
    s.as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|p| u8::from_str_radix(std::str::from_utf8(p).unwrap(), 16).unwrap())
        .collect()
}

#[test]
fn files_match_independent_cipher_and_checksum_controls() {
    let controls: Value =
        serde_json::from_str(include_str!("fixtures/complete_files.json")).unwrap();
    let cipher = StorageCipher::new(&hex(controls["key"].as_str().unwrap()).try_into().unwrap());
    for case in controls["cases"].as_array().unwrap() {
        let record = IndexRecord::parse(&hex(case["record"].as_str().unwrap())).unwrap();
        let logical = record.logical_size() as usize;
        let metadata_length = case["metadata_length"].as_u64().unwrap() as usize;
        let payload: Vec<_> = (0..logical)
            .map(|i| (i.wrapping_mul(37).wrapping_add(11) % 256) as u8)
            .collect();
        let metadata: Vec<_> = (0..metadata_length)
            .map(|i| (i.wrapping_mul(13).wrapping_add(9) % 256) as u8)
            .collect();
        let footer = hex(case["footer"].as_str().unwrap());
        let mut output = Vec::new();
        write_complete(
            &mut Cursor::new(&payload),
            &mut output,
            &record,
            &cipher,
            &metadata,
            footer[40..].try_into().unwrap(),
        )
        .unwrap();
        assert_eq!(
            format!("{:x}", Sha1::digest(&output)),
            case["encoded_sha1"].as_str().unwrap()
        );
        if record.flags() & 8 == 0 {
            cipher.transform_payload(
                &file_iv(record.file_id(), record.realm(), record.version()),
                0,
                &mut output,
            );
        }
        assert_eq!(&output[..logical], payload);
        assert_eq!(&output[output.len() - 64..], footer);
        let tail = &output[logical..];
        let trailer = Trailer::parse(&record, tail).unwrap();
        assert_eq!(trailer.metadata(), metadata);
        for (i, chunk) in payload.chunks(CHUNK_SIZE).enumerate() {
            trailer.validate_chunk(&record, i as u32, chunk).unwrap();
            let mut corrupt = chunk.to_vec();
            corrupt[0] ^= 1;
            assert!(trailer.validate_chunk(&record, i as u32, &corrupt).is_err());
        }
        let mut corrupt = tail.to_vec();
        *corrupt.last_mut().unwrap() ^= 1;
        assert!(Trailer::parse(&record, &corrupt).is_err());
        if !metadata.is_empty() {
            let mut corrupt = tail.to_vec();
            corrupt[0] ^= 1;
            assert!(Trailer::parse(&record, &corrupt).is_err());
        }
        if logical > 0 {
            assert_eq!(
                write_complete(
                    &mut io::empty(),
                    &mut Vec::new(),
                    &record,
                    &cipher,
                    &metadata,
                    &[0; 24]
                )
                .unwrap_err()
                .kind(),
                io::ErrorKind::UnexpectedEof
            );
        }
    }
    assert!(stored_size(u32::MAX, 0).is_err());
    assert!(stored_size(1, 0x100_0000).is_err());
}
