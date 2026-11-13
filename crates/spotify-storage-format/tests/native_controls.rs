use serde_json::Value;
use spotify_storage_format::*;

fn hex(value: &str) -> Vec<u8> {
    assert_eq!(value.len() % 2, 0);
    value
        .as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}
fn bytes(row: &Value, key: &str) -> Vec<u8> {
    hex(row[key].as_str().unwrap())
}
fn controls() -> Value {
    serde_json::from_str(include_str!("fixtures/native_transforms.json")).unwrap()
}

#[test]
fn index_bytes_match_native_writer_and_reader() {
    let fixtures = controls();
    let key: [u8; 16] = bytes(&fixtures, "test_key").try_into().unwrap();
    let cipher = StorageCipher::new(&key);
    for row in fixtures["records"].as_array().unwrap() {
        let slot = row["slot"].as_u64().unwrap() as u32;
        let record = IndexRecord::parse(&bytes(row, "plain")).unwrap();
        let expected = bytes(row, "cipher");
        assert_eq!(
            cipher.encode_record(slot, &record).as_slice(),
            expected,
            "slot {slot}"
        );
        assert!(cipher.decode_record(slot, &expected).unwrap() == record);
        assert!(
            cipher
                .decode_record(slot.wrapping_add(1), &expected)
                .is_err()
        );
    }
}

#[test]
fn payload_bytes_and_seek_offsets_match_native() {
    let fixtures = controls();
    let cipher = StorageCipher::new(&bytes(&fixtures, "test_key").try_into().unwrap());
    for row in fixtures["payload"].as_array().unwrap() {
        let iv = bytes(row, "iv").try_into().unwrap();
        let offset = row["offset"].as_u64().unwrap();
        let mut actual = bytes(row, "plain");
        cipher.transform_payload(&iv, offset, &mut actual);
        assert_eq!(actual, bytes(row, "cipher"), "offset {offset}");
        cipher.transform_payload(&iv, offset, &mut actual);
        assert_eq!(actual, bytes(row, "plain"));
        let (left, right) = actual.split_at_mut(7);
        cipher.transform_payload(&iv, offset, left);
        cipher.transform_payload(&iv, offset + 7, right);
        assert_eq!(actual, bytes(row, "cipher"));
    }
}

#[test]
fn per_file_state_matches_native_storage_transform() {
    let fixtures = controls();
    let cipher = StorageCipher::new(&bytes(&fixtures, "test_key").try_into().unwrap());
    for row in fixtures["files"].as_array().unwrap() {
        let file_id = bytes(row, "file_id").try_into().unwrap();
        let iv = file_iv(
            &file_id,
            row["realm"].as_u64().unwrap() as u8,
            row["version"].as_u64().unwrap() as u16,
        );
        let mut actual = bytes(row, "plain");
        cipher.transform_payload(&iv, row["offset"].as_u64().unwrap(), &mut actual);
        assert_eq!(actual, bytes(row, "cipher"));
    }
}

#[test]
fn names_match_native_with_explicit_state() {
    let fixtures = controls();
    for row in fixtures["names"].as_array().unwrap() {
        let name = cache_file_name(
            &bytes(row, "state").try_into().unwrap(),
            &bytes(row, "file_id").try_into().unwrap(),
            row["realm"].as_u64().unwrap() as u8,
            row["version"].as_u64().unwrap() as u16,
        );
        assert_eq!(name, row["name"].as_str().unwrap());
    }
}

#[test]
fn capacity_arithmetic_matches_161_native_cases() {
    let fixtures: Value =
        serde_json::from_str(include_str!("fixtures/native_capacity.json")).unwrap();
    let rows = fixtures["vectors"].as_array().unwrap();
    assert_eq!(rows.len(), 161);
    for row in rows {
        let input = row["input"].as_array().unwrap();
        let config = CapacityConfig {
            absolute_a: input[0].as_i64().unwrap(),
            fraction_a: input[1].as_f64().unwrap() as f32,
            fraction_b: input[2].as_f64().unwrap() as f32,
            absolute_b: input[3].as_i64().unwrap(),
        };
        let actual = capacity_values(config, input[4].as_u64());
        let expected = row["output"].as_array().unwrap();
        assert_eq!(actual.first, expected[0].as_i64().unwrap());
        assert_eq!(actual.second, expected[1].as_i64().unwrap());
        assert_eq!(actual.small_disk_bound, expected[2].as_u64().unwrap());
        assert_eq!(actual.large_disk_bound, expected[3].as_u64().unwrap());
    }
}

#[test]
fn updates_preserve_reserved_bytes_and_reject_corruption() {
    let fixtures = controls();
    let original = IndexRecord::parse(&bytes(&fixtures["records"][0], "plain")).unwrap();
    let changed = original.with_stored_size(1234567);
    assert_eq!(changed.stored_size(), 1234567);
    assert_eq!(&changed.as_bytes()[4..44], &original.as_bytes()[4..44]);
    assert_eq!(&changed.as_bytes()[48..], &original.as_bytes()[48..]);
    assert!(IndexRecord::parse(changed.as_bytes()).is_ok());
    let mut broken = *changed.as_bytes();
    broken[63] ^= 1;
    assert!(matches!(
        IndexRecord::parse(&broken),
        Err(FormatError::RecordChecksum)
    ));
    assert!(matches!(
        IndexRecord::parse(&broken[..63]),
        Err(FormatError::RecordLength)
    ));
}

#[test]
fn settings_preserve_exact_bytes_only_with_consistent_legacy_value() {
    assert_eq!(configured_limit_bytes(2, "1048577"), 1048577);
    assert_eq!(configured_limit_bytes(1, "1048577"), 1048576);
    for invalid in [
        "",
        " 1048577",
        "1048577 ",
        "+1048577",
        "-1",
        "18446744073709551615",
    ] {
        assert_eq!(configured_limit_bytes(2, invalid), 2097152);
    }
    assert_eq!(configured_limit_bytes(-1, "0"), -1048576);
}

#[test]
fn checksum_supports_independent_standard_vector_and_incremental_reads() {
    assert_eq!(adler32(1, b"Wikipedia"), 0x11e60398);
    let data: Vec<u8> = (0..20000).map(|i| i as u8).collect();
    assert_eq!(
        adler32(1, &data),
        adler32(adler32(1, &data[..7777]), &data[7777..])
    );
}
