//! Pure data model + grid logic. Deliberately free of any HWND/COM types so it
//! can be unit-tested without a message loop.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Current on-disk schema version. Bump when the format changes and add a
/// migration in [`crate::storage`].
pub const SCHEMA_VERSION: u32 = 1;

/// Standard icon sizes offered in the UI (device-independent pixels).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IconSize {
    Small = 32,
    Medium = 48,
    Large = 64,
    Huge = 96,
}

impl IconSize {
    pub fn from_px(px: u32) -> IconSize {
        match px {
            0..=39 => IconSize::Small,
            40..=55 => IconSize::Medium,
            56..=79 => IconSize::Large,
            _ => IconSize::Huge,
        }
    }
    pub fn px(self) -> u32 {
        self as u32
    }
    pub const ALL: [IconSize; 4] = [
        IconSize::Small,
        IconSize::Medium,
        IconSize::Large,
        IconSize::Huge,
    ];
    pub fn label(self) -> &'static str {
        match self {
            IconSize::Small => "Small",
            IconSize::Medium => "Medium",
            IconSize::Large => "Large",
            IconSize::Huge => "Huge",
        }
    }
}

/// Saved window position, including which monitor it was on.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowPosition {
    pub x: i32,
    pub y: i32,
    #[serde(default)]
    pub monitor: String,
}

/// A single shortcut inside a drawer. `shortcut` is a path *relative to the
/// drawer directory* (e.g. `Items/<uuid>.lnk`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Item {
    pub id: Uuid,
    pub shortcut: String,
    pub display_name: String,
    pub column: u32,
    pub row: u32,
}

/// A drawer: a fixed grid of shortcut items plus its window/layout settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Drawer {
    #[serde(default = "default_schema")]
    pub schema_version: u32,
    pub id: Uuid,
    pub name: String,
    pub columns: u32,
    pub rows: u32,
    pub icon_size: u32,
    #[serde(default = "default_true")]
    pub show_labels: bool,
    #[serde(default)]
    pub layout_locked: bool,
    #[serde(default)]
    pub window_position: Option<WindowPosition>,
    #[serde(default)]
    pub items: Vec<Item>,
}

fn default_schema() -> u32 {
    SCHEMA_VERSION
}
fn default_true() -> bool {
    true
}

impl Drawer {
    /// Create a fresh, empty drawer.
    pub fn new(name: impl Into<String>, columns: u32, rows: u32, icon_size: IconSize) -> Drawer {
        Drawer {
            schema_version: SCHEMA_VERSION,
            id: Uuid::new_v4(),
            name: name.into(),
            columns: columns.max(1),
            rows: rows.max(1),
            icon_size: icon_size.px(),
            show_labels: true,
            layout_locked: false,
            window_position: None,
            items: Vec::new(),
        }
    }

    /// Is the given cell occupied by some item?
    fn cell_occupied(&self, col: u32, row: u32) -> bool {
        self.items.iter().any(|i| i.column == col && i.row == row)
    }

    /// Find the next free cell scanning left-to-right, top-to-bottom.
    pub fn next_free_cell(&self) -> Option<(u32, u32)> {
        for row in 0..self.rows {
            for col in 0..self.columns {
                if !self.cell_occupied(col, row) {
                    return Some((col, row));
                }
            }
        }
        None
    }

    /// Add an item into the next free cell. Returns [`crate::error::Error::GridFull`]
    /// if there is no room.
    pub fn add_item(&mut self, shortcut: String, display_name: String, id: Uuid) -> crate::error::Result<()> {
        let (col, row) = self.next_free_cell().ok_or(crate::error::Error::GridFull)?;
        self.items.push(Item {
            id,
            shortcut,
            display_name,
            column: col,
            row,
        });
        Ok(())
    }

    pub fn remove_item(&mut self, id: Uuid) -> Option<Item> {
        if let Some(pos) = self.items.iter().position(|i| i.id == id) {
            Some(self.items.remove(pos))
        } else {
            None
        }
    }

    /// Reflow items into cells filling left-to-right, preserving current order.
    pub fn fill_left_to_right(&mut self) {
        let cols = self.columns;
        for (idx, item) in self.items.iter_mut().enumerate() {
            let idx = idx as u32;
            item.column = idx % cols;
            item.row = idx / cols;
        }
    }

    /// Reflow items filling top-to-bottom, column by column.
    pub fn fill_top_to_bottom(&mut self) {
        let rows = self.rows;
        for (idx, item) in self.items.iter_mut().enumerate() {
            let idx = idx as u32;
            item.column = idx / rows;
            item.row = idx % rows;
        }
    }

    /// Sort items alphabetically by display name, then reflow left-to-right.
    pub fn sort_by_name(&mut self) {
        self.items
            .sort_by_key(|a| a.display_name.to_lowercase());
        self.fill_left_to_right();
    }

    /// Remove gaps by re-packing items in reading order (stable by cell).
    pub fn remove_empty_spaces(&mut self) {
        self.items.sort_by_key(|i| (i.row, i.column));
        self.fill_left_to_right();
    }

    /// Duplicate this drawer with a new id and name. Items keep their relative
    /// shortcut paths; the caller is responsible for copying the actual `.lnk`
    /// files into the new drawer directory.
    pub fn duplicated(&self, new_name: impl Into<String>) -> Drawer {
        let mut d = self.clone();
        d.id = Uuid::new_v4();
        d.name = new_name.into();
        d.window_position = None;
        d
    }
}

/// The top-level `drawers.json` index: order + ids.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DrawerIndex {
    #[serde(default = "default_schema")]
    pub schema_version: u32,
    #[serde(default)]
    pub drawer_order: Vec<Uuid>,
}

impl Default for DrawerIndex {
    fn default() -> Self {
        DrawerIndex {
            schema_version: SCHEMA_VERSION,
            drawer_order: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drawer() -> Drawer {
        Drawer::new("Test", 3, 2, IconSize::Medium)
    }

    #[test]
    fn fills_next_free_cell() {
        let mut d = drawer();
        d.add_item("a.lnk".into(), "A".into(), Uuid::new_v4()).unwrap();
        d.add_item("b.lnk".into(), "B".into(), Uuid::new_v4()).unwrap();
        assert_eq!((d.items[0].column, d.items[0].row), (0, 0));
        assert_eq!((d.items[1].column, d.items[1].row), (1, 0));
    }

    #[test]
    fn grid_full_errors() {
        let mut d = Drawer::new("Tiny", 1, 1, IconSize::Small);
        d.add_item("a.lnk".into(), "A".into(), Uuid::new_v4()).unwrap();
        assert!(d.add_item("b.lnk".into(), "B".into(), Uuid::new_v4()).is_err());
    }

    #[test]
    fn sort_by_name_orders_items() {
        let mut d = drawer();
        d.add_item("z.lnk".into(), "Zebra".into(), Uuid::new_v4()).unwrap();
        d.add_item("a.lnk".into(), "Apple".into(), Uuid::new_v4()).unwrap();
        d.sort_by_name();
        assert_eq!(d.items[0].display_name, "Apple");
        assert_eq!((d.items[0].column, d.items[0].row), (0, 0));
    }

    #[test]
    fn duplicate_gets_new_id() {
        let d = drawer();
        let dup = d.duplicated("Copy");
        assert_ne!(d.id, dup.id);
        assert_eq!(dup.name, "Copy");
    }

    #[test]
    fn icon_size_roundtrips() {
        assert_eq!(IconSize::from_px(48), IconSize::Medium);
        assert_eq!(IconSize::Large.px(), 64);
    }
}
