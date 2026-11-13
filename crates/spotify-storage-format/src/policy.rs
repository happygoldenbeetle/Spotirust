//! Pure cleanup planning reconstructed from Windows client 1.3.8.620.
//! This module never opens files and never performs deletions.
use crate::{CapacityConfig, CapacityValues, sse_truncate};
use std::collections::HashMap;

pub const TICKS_PER_SECOND: i64 = 10_000_000;
pub const REALMS: [u8; 14] = [0, 1, 4, 5, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];

/// Observed initialized defaults for the reference version, before overrides.
pub const CAPACITY_DEFAULTS: CapacityConfig = CapacityConfig {
    absolute_a: 10 * 1024 * 1024 * 1024,
    fraction_a: 0.9,
    fraction_b: 0.03,
    absolute_b: 500 * 1024 * 1024,
};

#[derive(Clone, Copy, Debug)]
pub struct RealmConfig {
    pub priority: u32,
    pub fraction: f32,
    pub used_ttl: i64,
    pub new_ttl: i64,
    pub alternate_ttl: i64,
    pub minimum_after_alternate: i64,
    pub minimum_age: i64,
}

pub fn default_realm(realm: u8) -> Option<RealmConfig> {
    let (
        priority,
        fraction,
        used_ttl,
        new_ttl,
        alternate_ttl,
        minimum_after_alternate,
        minimum_age,
    ) = match realm {
        0 | 15 => (1, 0.0, 6_480_000, 604_800, 0, 129_600, 3600),
        1 => (0, 0.075, 1_209_600, 1_209_600, 0, 0, 3600),
        4 => (0, 0.1, 0, 0, 0, 0, 3600),
        5 | 8 | 10 | 16 => (1, 0.1, 604_800, 604_800, 3, 0, 3600),
        7 => (1, 0.1, 2_592_000, 86_400, 0, 0, 3600),
        9 => (1, 0.0, 108_000, 604_800, 0, 0, 3600),
        11 => (1, 0.05, 86_400, 86_400, 3, 0, 600),
        12 | 13 => (0, 0.1, 604_800, 604_800, 3, 0, 3600),
        14 => (0, 0.1, 72_000, 72_000, 0, 0, 3600),
        _ => return None,
    };
    Some(RealmConfig {
        priority,
        fraction,
        used_ttl,
        new_ttl,
        alternate_ttl,
        minimum_after_alternate,
        minimum_age,
    })
}

#[derive(Clone, Copy, Debug)]
pub struct Entry {
    pub realm: u8,
    pub created: u32,
    pub used: u32,
    /// Preserves the native field without assigning it an unverified user meaning.
    pub alternate: u32,
    pub stored_size: u32,
    pub open_handles: u64,
    pub protection: u8,
}

pub fn retention_deadline(
    entry: &Entry,
    config: Option<&RealmConfig>,
    ignore_open_handles: bool,
) -> Option<i64> {
    if (!ignore_open_handles && entry.open_handles != 0)
        || entry.protection != 0
        || entry.created == 0
    {
        return None;
    }
    let config = config?;
    if config.used_ttl == 0 {
        return None;
    }
    let alternate = i64::from(entry.alternate);
    let mut deadline = if entry.used == 0 {
        i64::from(entry.created).wrapping_add(config.new_ttl)
    } else {
        i64::from(entry.used).wrapping_add(config.used_ttl)
    };
    if alternate != 0 {
        if entry.used != 0 && entry.alternate > entry.used && config.alternate_ttl > 0 {
            deadline = deadline.min(alternate.wrapping_add(config.alternate_ttl));
        } else if config.minimum_after_alternate > 0 {
            deadline = deadline.max(alternate.wrapping_add(config.minimum_after_alternate));
        }
    }
    Some(deadline.wrapping_mul(TICKS_PER_SECOND))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum Reason {
    ExpiredUsed = 0,
    ExpiredUnused = 1,
    Capacity = 2,
    RealmQuota = 3,
    FreeSpace = 4,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Selection {
    pub entry: usize,
    pub reason: Reason,
}

/// Plans only. The caller supplies native-compatible realm overrides, current
/// time and local midnight in Unix 100 ns ticks, and disk capacity values.
/// The sorter preserves the native algorithm's ordering when deadlines tie.
pub fn plan_eviction(
    entries: &[Entry],
    configs: &HashMap<u8, RealmConfig>,
    capacity: CapacityValues,
    now_ticks: i64,
    local_midnight_ticks: i64,
    free_bytes: u64,
) -> Vec<Selection> {
    let mut sizes: HashMap<u8, i128> = HashMap::new();
    let mut protected: HashMap<u8, i128> = HashMap::new();
    let mut protected_total = 0;
    for entry in entries {
        *sizes.entry(entry.realm).or_default() += i128::from(entry.stored_size);
        if entry.protection != 0 {
            *protected.entry(entry.realm).or_default() += i128::from(entry.stored_size);
            protected_total += i128::from(entry.stored_size);
        }
    }
    let mut quota = HashMap::new();
    for &realm in &REALMS {
        if let Some(config) = configs.get(&realm)
            && config.fraction != 0.0
        {
            let size = sizes.get(&realm).copied().unwrap_or_default();
            let protected = protected.get(&realm).copied().unwrap_or_default().min(size);
            let target = i128::from(sse_truncate(capacity.first as f32 * config.fraction));
            let excess = size - protected - target;
            if excess > 0 {
                quota.insert(realm, excess);
            }
        }
    }
    let total: i128 = REALMS
        .iter()
        .map(|realm| sizes.get(realm).copied().unwrap_or_default())
        .sum();
    let mut excess = total - i128::from(capacity.first) - protected_total;
    let mut free = i128::from(free_bytes);
    let mut selected = Vec::new();
    let mut candidates = Vec::new();
    for (index, entry) in entries.iter().enumerate() {
        let config = configs.get(&entry.realm);
        let Some(deadline) = retention_deadline(entry, config, false) else {
            continue;
        };
        let config = config.unwrap();
        let expiry_now = if config.used_ttl >= 86_401 {
            local_midnight_ticks
        } else {
            now_ticks
        };
        if deadline < expiry_now {
            selected.push(Selection {
                entry: index,
                reason: if entry.used == 0 {
                    Reason::ExpiredUnused
                } else {
                    Reason::ExpiredUsed
                },
            });
            let size = i128::from(entry.stored_size);
            excess -= size;
            free += size;
            if let Some(quota) = quota.get_mut(&entry.realm) {
                *quota -= size;
            }
        } else if i64::from(entry.created).wrapping_mul(TICKS_PER_SECOND)
            < now_ticks.wrapping_sub(config.minimum_age.wrapping_mul(TICKS_PER_SECOND))
        {
            candidates.push((index, deadline));
        }
    }
    crate::policy_sort::sort(&mut candidates);
    let mut remaining = Vec::with_capacity(candidates.len());
    for (index, _) in candidates {
        let entry = &entries[index];
        if let Some(quota_left) = quota.get_mut(&entry.realm) {
            selected.push(Selection {
                entry: index,
                reason: Reason::RealmQuota,
            });
            let size = i128::from(entry.stored_size);
            excess -= size;
            free += size;
            *quota_left -= size;
            if *quota_left <= 0 {
                quota.remove(&entry.realm);
            }
        } else {
            remaining.push(index);
        }
    }
    for index in remaining {
        let reason = if excess > 0 {
            Some(Reason::Capacity)
        } else if free < i128::from(capacity.second) {
            Some(Reason::FreeSpace)
        } else {
            None
        };
        if let Some(reason) = reason {
            selected.push(Selection {
                entry: index,
                reason,
            });
            let size = i128::from(entries[index].stored_size);
            excess -= size;
            free += size;
        }
    }
    selected
}
