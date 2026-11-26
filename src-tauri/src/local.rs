//! Settings that belong to this PC only (folders, the PC's identity). They
//! live in `meta`, never in the synced documents: each PC has its own
//! backup folder and its own path to the sync folder.

use crate::db::{self, err, R};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::hash::{BuildHasher, Hasher};

const KEY: &str = "local";

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Local {
    pub device_id: String,
    pub device_name: String,
    /// Daily automatic backup (on unless turned off).
    pub backup_off: bool,
    /// Empty = the default folder (Documentos\Nihongo Michi\Backups).
    pub backup_dir: String,
    /// How many daily backups to keep (0 = default).
    pub backup_keep: u32,
    /// Shared folder for the sync between PCs. Empty = sync off.
    pub sync_dir: String,
    /// Do not look for new versions by itself.
    pub update_check_off: bool,
    /// Ask before installing (never install by itself when the window closes).
    pub update_install_ask: bool,
}

pub const DEFAULT_KEEP: u32 = 30;

impl Local {
    pub fn keep(&self) -> usize {
        (if self.backup_keep == 0 { DEFAULT_KEEP } else { self.backup_keep.clamp(3, 365) }) as usize
    }
    pub fn sync_on(&self) -> bool {
        !self.sync_dir.trim().is_empty()
    }
}

fn random_id() -> String {
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u128((michi_core::time::now_ms().to_bits() as u128) ^ ((std::process::id() as u128) << 64));
    format!("{:08x}", h.finish() as u32)
}

fn computer_name() -> Option<String> {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// This PC's settings; gives the PC an id and a name the first time. A
/// database copied from another PC gets a new identity here, so the two
/// never write the same file in the sync folder.
pub fn get(c: &Connection) -> Local {
    let mut l: Local = db::meta_get(c, KEY).and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
    let name = computer_name();
    let moved = matches!(&name, Some(n) if !l.device_name.is_empty() && *n != l.device_name);
    if l.device_id.is_empty() || l.device_name.is_empty() || moved {
        l.device_id = if l.device_id.is_empty() || moved { random_id() } else { l.device_id };
        l.device_name = name.unwrap_or_else(|| "PC".into());
        let _ = set(c, &l);
    }
    l
}

pub fn set(c: &Connection, l: &Local) -> R<()> {
    db::meta_set(c, KEY, &serde_json::to_string(l).map_err(err)?)
}

pub fn update(c: &Connection, f: impl FnOnce(&mut Local)) -> R<Local> {
    let mut l = get(c);
    f(&mut l);
    set(c, &l)?;
    Ok(l)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;

    #[test]
    fn identity_is_created_once() {
        let db = Db::memory();
        let c = db.lock();
        let a = get(&c);
        assert_eq!(a.device_id.len(), 8);
        assert!(!a.device_name.is_empty());
        assert_eq!(get(&c).device_id, a.device_id);
        assert_eq!(a.keep(), 30);
        let b = update(&c, |l| l.backup_keep = 1).unwrap();
        assert_eq!(b.keep(), 3);
        assert!(!b.sync_on());
    }
}
