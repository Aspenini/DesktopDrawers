//! Configuration persistence: directory layout, atomic writes, backups, and
//! schema migration. Independent of any window/COM types.

use std::fs;
use std::path::{Path, PathBuf};

use uuid::Uuid;

use crate::error::{Error, Result};
use crate::model::{Drawer, DrawerIndex, SCHEMA_VERSION};

/// Resolves and owns the `%LOCALAPPDATA%\DesktopDrawers` directory tree.
#[derive(Debug, Clone)]
pub struct Store {
    root: PathBuf,
}

impl Store {
    /// Open the default store under `%LOCALAPPDATA%\DesktopDrawers`, creating
    /// the directory tree if needed.
    pub fn open_default() -> Result<Store> {
        let base = local_app_data()?.join("DesktopDrawers");
        Store::open_at(base)
    }

    /// Open a store rooted at an explicit directory (used by tests).
    pub fn open_at(root: impl Into<PathBuf>) -> Result<Store> {
        let store = Store { root: root.into() };
        for dir in [
            store.data_dir(),
            store.drawers_dir(),
            store.cache_icons_dir(),
            store.logs_dir(),
        ] {
            fs::create_dir_all(&dir).map_err(|e| Error::io(&dir, e))?;
        }
        Ok(store)
    }

    pub fn data_dir(&self) -> PathBuf {
        self.root.join("Data")
    }
    pub fn drawers_dir(&self) -> PathBuf {
        self.data_dir().join("Drawers")
    }
    pub fn cache_icons_dir(&self) -> PathBuf {
        self.root.join("Cache").join("Icons")
    }
    pub fn logs_dir(&self) -> PathBuf {
        self.root.join("Logs")
    }
    pub fn index_path(&self) -> PathBuf {
        self.data_dir().join("drawers.json")
    }
    pub fn drawer_dir(&self, id: Uuid) -> PathBuf {
        self.drawers_dir().join(id.to_string())
    }
    pub fn drawer_json_path(&self, id: Uuid) -> PathBuf {
        self.drawer_dir(id).join("drawer.json")
    }
    pub fn drawer_items_dir(&self, id: Uuid) -> PathBuf {
        self.drawer_dir(id).join("Items")
    }

    // ---- Index ----------------------------------------------------------

    pub fn load_index(&self) -> Result<DrawerIndex> {
        let path = self.index_path();
        match read_json_with_backup::<DrawerIndex>(&path) {
            Ok(Some(mut idx)) => {
                check_schema(idx.schema_version)?;
                idx.schema_version = SCHEMA_VERSION;
                Ok(idx)
            }
            Ok(None) => Ok(DrawerIndex::default()),
            Err(e) => Err(e),
        }
    }

    pub fn save_index(&self, index: &DrawerIndex) -> Result<()> {
        let json = serde_json::to_vec_pretty(index)?;
        atomic_write(&self.index_path(), &json)
    }

    // ---- Drawers --------------------------------------------------------

    pub fn load_drawer(&self, id: Uuid) -> Result<Drawer> {
        let path = self.drawer_json_path(id);
        match read_json_with_backup::<Drawer>(&path)? {
            Some(mut d) => {
                check_schema(d.schema_version)?;
                d.schema_version = SCHEMA_VERSION;
                Ok(d)
            }
            None => Err(Error::DrawerNotFound(id.to_string())),
        }
    }

    pub fn save_drawer(&self, drawer: &Drawer) -> Result<()> {
        let dir = self.drawer_dir(drawer.id);
        let items = self.drawer_items_dir(drawer.id);
        fs::create_dir_all(&items).map_err(|e| Error::io(&items, e))?;
        let _ = dir; // items dir creation also creates the drawer dir
        let json = serde_json::to_vec_pretty(drawer)?;
        atomic_write(&self.drawer_json_path(drawer.id), &json)
    }

    /// Load every drawer listed in the index, in order. Drawers that fail to
    /// load are skipped (logged by the caller) rather than aborting startup.
    pub fn load_all(&self) -> Result<(DrawerIndex, Vec<Drawer>)> {
        let index = self.load_index()?;
        let mut drawers = Vec::new();
        for id in &index.drawer_order {
            match self.load_drawer(*id) {
                Ok(d) => drawers.push(d),
                Err(_) => { /* skip; caller may prune the index */ }
            }
        }
        Ok((index, drawers))
    }

    /// Permanently delete a drawer's directory (its managed `.lnk` files only —
    /// never the shortcut targets).
    pub fn delete_drawer_dir(&self, id: Uuid) -> Result<()> {
        let dir = self.drawer_dir(id);
        if dir.exists() {
            fs::remove_dir_all(&dir).map_err(|e| Error::io(&dir, e))?;
        }
        Ok(())
    }
}

/// Resolve `%LOCALAPPDATA%`.
fn local_app_data() -> Result<PathBuf> {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .ok_or_else(|| Error::Other("LOCALAPPDATA is not set".into()))
}

fn check_schema(found: u32) -> Result<()> {
    if found > SCHEMA_VERSION {
        Err(Error::UnsupportedSchema {
            found,
            supported: SCHEMA_VERSION,
        })
    } else {
        Ok(())
    }
}

/// Atomic write: temp file -> flush -> rotate old to `.bak` -> rename temp.
///
/// On Windows, `fs::rename` cannot overwrite an existing file, so we remove the
/// destination after backing it up.
pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;
    }
    let tmp = path.with_extension("tmp");
    {
        use std::io::Write;
        let mut f = fs::File::create(&tmp).map_err(|e| Error::io(&tmp, e))?;
        f.write_all(bytes).map_err(|e| Error::io(&tmp, e))?;
        f.sync_all().map_err(|e| Error::io(&tmp, e))?;
    }

    // Keep a single backup of the previous good file.
    if path.exists() {
        let bak = path.with_extension("bak");
        let _ = fs::remove_file(&bak);
        // Copy (not rename) so a crash mid-swap still leaves `path` intact.
        fs::copy(path, &bak).map_err(|e| Error::io(&bak, e))?;
        fs::remove_file(path).map_err(|e| Error::io(path, e))?;
    }

    fs::rename(&tmp, path).map_err(|e| Error::io(path, e))?;
    Ok(())
}

/// Read + parse JSON, transparently falling back to a `.bak` sibling if the
/// primary file is missing or corrupt. Returns `Ok(None)` if neither exists.
fn read_json_with_backup<T: serde::de::DeserializeOwned>(path: &Path) -> Result<Option<T>> {
    match fs::read(path) {
        Ok(bytes) => match serde_json::from_slice::<T>(&bytes) {
            Ok(v) => Ok(Some(v)),
            Err(primary_err) => {
                // Primary is corrupt; try the backup.
                let bak = path.with_extension("bak");
                if let Ok(bak_bytes) = fs::read(&bak)
                    && let Ok(v) = serde_json::from_slice::<T>(&bak_bytes) {
                        // Restore the good backup over the corrupt primary.
                        let _ = atomic_write(path, &bak_bytes);
                        return Ok(Some(v));
                    }
                Err(Error::Json(primary_err))
            }
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // Primary missing; maybe a backup survived.
            let bak = path.with_extension("bak");
            match fs::read(&bak) {
                Ok(bak_bytes) => {
                    let v = serde_json::from_slice::<T>(&bak_bytes)?;
                    let _ = atomic_write(path, &bak_bytes);
                    Ok(Some(v))
                }
                Err(_) => Ok(None),
            }
        }
        Err(e) => Err(Error::io(path, e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::IconSize;

    fn temp_store() -> (Store, PathBuf) {
        let mut dir = std::env::temp_dir();
        dir.push(format!("dd-test-{}", Uuid::new_v4()));
        (Store::open_at(&dir).unwrap(), dir)
    }

    #[test]
    fn save_and_load_drawer_roundtrips() {
        let (store, dir) = temp_store();
        let mut d = Drawer::new("Emulation", 4, 3, IconSize::Medium);
        d.add_item("Items/x.lnk".into(), "RPCS3".into(), Uuid::new_v4()).unwrap();
        store.save_drawer(&d).unwrap();
        let loaded = store.load_drawer(d.id).unwrap();
        assert_eq!(loaded.name, "Emulation");
        assert_eq!(loaded.items.len(), 1);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn atomic_write_keeps_backup() {
        let (store, dir) = temp_store();
        let p = store.data_dir().join("thing.json");
        atomic_write(&p, b"{\"a\":1}").unwrap();
        atomic_write(&p, b"{\"a\":2}").unwrap();
        let bak = p.with_extension("bak");
        assert!(bak.exists(), "backup should exist after second write");
        assert_eq!(fs::read(&bak).unwrap(), b"{\"a\":1}");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn corrupt_primary_restores_from_backup() {
        let (store, dir) = temp_store();
        let idx = DrawerIndex::default();
        store.save_index(&idx).unwrap();
        store.save_index(&idx).unwrap(); // creates .bak
        // Corrupt the primary.
        fs::write(store.index_path(), b"not json at all").unwrap();
        let recovered = store.load_index().unwrap();
        assert_eq!(recovered.schema_version, SCHEMA_VERSION);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn newer_schema_is_rejected() {
        let (store, dir) = temp_store();
        let json = format!("{{\"schema_version\":{},\"drawer_order\":[]}}", SCHEMA_VERSION + 1);
        fs::write(store.index_path(), json).unwrap();
        assert!(matches!(store.load_index(), Err(Error::UnsupportedSchema { .. })));
        let _ = fs::remove_dir_all(dir);
    }
}
