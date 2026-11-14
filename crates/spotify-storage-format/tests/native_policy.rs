use serde_json::Value;
use spotify_storage_format::{CapacityValues, policy::*};
use std::collections::HashMap;

fn controls() -> Value {
    serde_json::from_str(include_str!("fixtures/native_policy.json")).unwrap()
}

#[test]
fn capacity_attribute_matches_128_native_property_parses() {
    let controls: Value =
        serde_json::from_str(include_str!("fixtures/native_capacity_attribute.json")).unwrap();
    for control in controls.as_array().unwrap() {
        let actual = capacity_attribute(control["input"].as_str());
        let expected = &control["result"];
        if expected.is_null() {
            assert!(actual.is_none(), "{control}");
        } else {
            let actual = actual.unwrap();
            assert_eq!(
                actual.absolute_a,
                expected["absolute_a"].as_i64().unwrap(),
                "{control}"
            );
            assert_eq!(
                actual.absolute_b,
                expected["absolute_b"].as_i64().unwrap(),
                "{control}"
            );
            assert_eq!(
                actual.fraction_a.to_bits(),
                (expected["fraction_a"].as_f64().unwrap() as f32).to_bits(),
                "{control}"
            );
            assert_eq!(
                actual.fraction_b.to_bits(),
                (expected["fraction_b"].as_f64().unwrap() as f32).to_bits(),
                "{control}"
            );
        }
    }
}

#[test]
fn closed_access_matches_64_native_timestamp_and_counter_updates() {
    use spotify_storage_format::{INDEX_RECORD_SIZE, IndexRecord, adler32};
    let controls: Value =
        serde_json::from_str(include_str!("fixtures/native_access.json")).unwrap();
    for control in controls.as_array().unwrap() {
        let mut raw = [0xa5; INDEX_RECORD_SIZE];
        raw[4..8].copy_from_slice(&0xdeadbeef_u32.to_be_bytes());
        raw[40..44].copy_from_slice(&1234_u32.to_be_bytes());
        raw[52..56].copy_from_slice(&5678_u32.to_be_bytes());
        raw[48] = unsigned(control, "before_count") as u8;
        let checksum = adler32(1, &raw[4..]);
        raw[..4].copy_from_slice(&checksum.to_be_bytes());
        let updated = IndexRecord::parse(&raw).unwrap().with_closed_access(
            unsigned(control, "seconds") as u32,
            control["read"].as_bool().unwrap(),
            control["write"].as_bool().unwrap(),
        );
        assert_eq!(updated.created(), unsigned(control, "created") as u32);
        assert_eq!(updated.last_used(), unsigned(control, "used") as u32);
        assert_eq!(updated.access_count(), unsigned(control, "count") as u8);
        for (offset, byte) in raw.iter().enumerate().skip(4) {
            if !(40..44).contains(&offset) && !(52..56).contains(&offset) && offset != 48 {
                assert_eq!(updated.as_bytes()[offset], *byte);
            }
        }
        assert!(IndexRecord::parse(updated.as_bytes()).is_ok());
    }
}

#[test]
fn cleanup_timing_matches_128_native_scheduled_tasks() {
    let controls: Value =
        serde_json::from_str(include_str!("fixtures/native_scheduling.json")).unwrap();
    for control in controls.as_array().unwrap() {
        assert_eq!(
            cleanup_delay_ms(
                control["removed"].as_bool().unwrap(),
                unsigned(control, "free"),
                unsigned(control, "reserve")
            ),
            unsigned(control, "delay_ms"),
            "{control}"
        );
    }
}
fn unsigned(v: &Value, field: &str) -> u64 {
    v[field].as_u64().unwrap_or_default()
}
fn entry(v: &Value) -> Entry {
    Entry {
        realm: unsigned(v, "realm") as u8,
        created: unsigned(v, "created") as u32,
        used: unsigned(v, "used") as u32,
        alternate: unsigned(v, "special") as u32,
        stored_size: unsigned(v, "size") as u32,
        open_handles: unsigned(v, "open_handles"),
        protection: unsigned(v, "protection") as u8,
    }
}

#[test]
fn initialized_realm_defaults_match_native_table() {
    let controls = controls();
    for native in controls["defaults"].as_array().unwrap() {
        let config = default_realm(unsigned(native, "realm") as u8).unwrap();
        assert_eq!(config.priority, unsigned(native, "priority") as u32);
        assert_eq!(
            config.fraction.to_bits(),
            (native["fraction"].as_f64().unwrap() as f32).to_bits()
        );
        let times: Vec<_> = native["times"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_i64().unwrap())
            .collect();
        assert_eq!(
            [
                config.used_ttl,
                config.new_ttl,
                config.alternate_ttl,
                config.minimum_after_alternate,
                config.minimum_age
            ]
            .as_slice(),
            times
        );
    }
    assert!(default_realm(255).is_none());
}

#[test]
fn retention_matches_350_native_decisions() {
    for row in controls()["retention"].as_array().unwrap() {
        let entry = entry(&row["entry"]);
        assert_eq!(
            retention_deadline(
                &entry,
                default_realm(entry.realm).as_ref(),
                row["ignore_open_handles"].as_bool().unwrap()
            ),
            row["deadline_ticks"].as_i64(),
            "{row}"
        );
    }
}

#[test]
fn eviction_plans_match_254_native_selections_without_deleting_anything() {
    let configs: HashMap<_, _> = REALMS
        .iter()
        .map(|&realm| (realm, default_realm(realm).unwrap()))
        .collect();
    for (index, row) in controls()["selection"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
    {
        let entries: Vec<_> = row["entries"]
            .as_array()
            .unwrap()
            .iter()
            .map(entry)
            .collect();
        let capacity = CapacityValues {
            first: row["capacity"][0].as_i64().unwrap(),
            second: row["capacity"][1].as_i64().unwrap(),
            small_disk_bound: 0,
            large_disk_bound: 0,
        };
        let plan = plan_eviction(
            &entries,
            &configs,
            capacity,
            row["now"].as_i64().unwrap() * TICKS_PER_SECOND,
            row["local_midnight"].as_i64().unwrap() * TICKS_PER_SECOND,
            unsigned(row, "free"),
        );
        let actual: Vec<_> = plan.iter().map(|s| (s.entry, s.reason as u32)).collect();
        let expected: Vec<_> = row["result"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| (unsigned(s, "entry") as usize, unsigned(s, "reason") as u32))
            .collect();
        assert_eq!(actual, expected, "native selection case {index}");
    }
}
