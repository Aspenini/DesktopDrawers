//! Integration tests for the non-GUI behaviors from IDEA.md §20.

use desktopdrawers::command::Command;
use desktopdrawers::model::{Drawer, IconSize};
use desktopdrawers::storage::Store;
use uuid::Uuid;

fn temp_store() -> (Store, std::path::PathBuf) {
    let mut dir = std::env::temp_dir();
    dir.push(format!("dd-it-{}", Uuid::new_v4()));
    (Store::open_at(&dir).unwrap(), dir)
}

#[test]
fn no_arguments_opens_manager() {
    assert_eq!(Command::parse(Vec::<String>::new()).unwrap(), Command::Manager);
    assert_eq!(Command::parse(["--manage"]).unwrap(), Command::Manager);
}

#[test]
fn open_flag_selects_drawer() {
    let id = "7dcf239e-5d4d-45af-8293-a354ee90d002";
    assert_eq!(
        Command::parse(["--open", id]).unwrap(),
        Command::OpenDrawer(id.into())
    );
}

#[test]
fn ipc_command_serialization_roundtrips() {
    for cmd in [
        Command::Manager,
        Command::OpenDrawer("abc-123".into()),
    ] {
        let json = cmd.to_ipc_json();
        assert_eq!(Command::from_ipc_json(&json).unwrap(), cmd);
    }
}

#[test]
fn drawer_lifecycle_create_save_load_duplicate_delete() {
    let (store, dir) = temp_store();

    // Create + save.
    let mut d = Drawer::new("Emulation", 4, 3, IconSize::Medium);
    d.add_item("Items/a.lnk".into(), "RPCS3".into(), Uuid::new_v4()).unwrap();
    d.add_item("Items/b.lnk".into(), "Xenia".into(), Uuid::new_v4()).unwrap();
    store.save_drawer(&d).unwrap();

    let mut index = store.load_index().unwrap();
    index.drawer_order.push(d.id);
    store.save_index(&index).unwrap();

    // Reload and verify (renaming shouldn't affect id / desktop shortcut).
    let mut loaded = store.load_drawer(d.id).unwrap();
    assert_eq!(loaded.name, "Emulation");
    assert_eq!(loaded.items.len(), 2);
    loaded.name = "Games".into();
    store.save_drawer(&loaded).unwrap();
    assert_eq!(store.load_drawer(d.id).unwrap().id, d.id, "id is stable across rename");

    // Duplicate gets a new id; original untouched.
    let dup = loaded.duplicated("Games (copy)");
    assert_ne!(dup.id, loaded.id);
    store.save_drawer(&dup).unwrap();
    assert!(store.load_drawer(dup.id).is_ok());
    assert!(store.load_drawer(loaded.id).is_ok());

    // Delete removes only the drawer's own directory.
    store.delete_drawer_dir(dup.id).unwrap();
    assert!(store.load_drawer(dup.id).is_err());
    assert!(store.load_drawer(loaded.id).is_ok());

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn load_all_returns_drawers_in_index_order() {
    let (store, dir) = temp_store();
    let mut index = store.load_index().unwrap();
    for name in ["One", "Two", "Three"] {
        let d = Drawer::new(name, 2, 2, IconSize::Small);
        store.save_drawer(&d).unwrap();
        index.drawer_order.push(d.id);
    }
    store.save_index(&index).unwrap();

    let (_idx, drawers) = store.load_all().unwrap();
    let names: Vec<_> = drawers.iter().map(|d| d.name.as_str()).collect();
    assert_eq!(names, ["One", "Two", "Three"]);

    let _ = std::fs::remove_dir_all(dir);
}
