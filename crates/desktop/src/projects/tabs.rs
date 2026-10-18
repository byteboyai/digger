//! 已打开的项目页签:有序 id 列表 + 当前页签,落盘为 `project_tabs.json`。
//!
//! 只存 id,不存路径与名字(那些以注册表为准)。读盘宽容、写盘原子;磁盘上是更高 schema 版本时用空状态显示且
//! **不回写**(与 `shell.json` 同一条规矩)。

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::Path;

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
struct FileShape {
    #[serde(default)]
    schema: u32,
    #[serde(default)]
    open: Vec<i64>,
    #[serde(default)]
    active: Option<i64>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ProjectTabs {
    open: Vec<i64>,
    active: Option<i64>,
}

/// 读盘结果:`save_blocked` 为真表示磁盘上是不支持的高版本文件,调用方不得回写。
#[derive(Debug, Clone, PartialEq)]
pub struct Loaded {
    pub tabs: ProjectTabs,
    pub save_blocked: bool,
}

impl ProjectTabs {
    pub fn open(&self) -> &[i64] {
        &self.open
    }

    pub fn active(&self) -> Option<i64> {
        self.active
    }

    /// 打开(或前台化)一个页签:已开着就不重复,只切为当前。
    pub fn open_tab(&mut self, id: i64) {
        if !self.open.contains(&id) {
            self.open.push(id);
        }
        self.active = Some(id);
    }

    /// 加一个页签但**不**把它变成当前(当前页签是第一张新开的页签时除外)。
    #[cfg(test)]
    #[allow(dead_code)] // 只在测试里调用
    pub fn add_tab(&mut self, id: i64) {
        if !self.open.contains(&id) {
            self.open.push(id);
        }
        if self.active.is_none() {
            self.active = Some(id);
        }
    }

    /// 关页签。关的是当前页签时,切到左邻;没有左邻则切到新的第一个;一个都不剩则无当前页签。
    pub fn close_tab(&mut self, id: i64) {
        let Some(at) = self.open.iter().position(|t| *t == id) else {
            return;
        };
        self.open.remove(at);
        if self.active == Some(id) {
            self.active = if at > 0 {
                self.open.get(at - 1).copied()
            } else {
                self.open.first().copied()
            };
        }
    }

    /// 把页签 `id` 插到 `index`(这是插入位,按"移走自己之后"的下标算);位置不变返回 `false`。
    pub fn move_tab(&mut self, id: i64, index: usize) -> bool {
        let Some(from) = self.open.iter().position(|t| *t == id) else {
            return false;
        };
        let mut open = self.open.clone();
        open.remove(from);
        let to = index.min(open.len());
        if to == from {
            return false;
        }
        open.insert(to, id);
        self.open = open;
        true
    }

    /// 切换当前页签;`id` 不在已开页签里返回 `false`,状态不变。
    pub fn activate(&mut self, id: i64) -> bool {
        if self.open.contains(&id) {
            self.active = Some(id);
            true
        } else {
            false
        }
    }

    /// 丢掉注册表里已不存在的项目(例如被取消登记),保持"当前页签必在已开页签里"。
    pub fn prune(&mut self, registered: &HashSet<i64>) {
        self.open.retain(|id| registered.contains(id));
        self.fix_active();
    }

    fn fix_active(&mut self) {
        if self.active.is_none_or(|a| !self.open.contains(&a)) {
            self.active = self.open.first().copied();
        }
    }

    fn sanitized(open: Vec<i64>, active: Option<i64>) -> Self {
        let mut seen = HashSet::new();
        let open: Vec<i64> = open.into_iter().filter(|id| seen.insert(*id)).collect();
        let mut tabs = Self { open, active };
        tabs.fix_active();
        tabs
    }

    /// 读盘。缺文件、读不出、不是 JSON 对象(含顶层数组:serde 会把 `[1,2]` 当位置序列塞进结构体)都回落为空状态。
    pub fn load(path: &Path) -> Loaded {
        let empty = Loaded {
            tabs: Self::default(),
            save_blocked: false,
        };
        let Ok(text) = std::fs::read_to_string(path) else {
            return empty;
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
            return empty;
        };
        if !value.is_object() {
            return empty;
        }
        let Ok(file) = serde_json::from_value::<FileShape>(value) else {
            return empty;
        };
        if file.schema > SCHEMA_VERSION {
            return Loaded {
                tabs: Self::default(),
                save_blocked: true,
            };
        }
        Loaded {
            tabs: Self::sanitized(file.open, file.active),
            save_blocked: false,
        }
    }

    /// 原子写盘:先写临时文件再改名,半截 JSON 不会被当成有效状态。
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let shape = FileShape {
            schema: SCHEMA_VERSION,
            open: self.open.clone(),
            active: self.active,
        };
        let tmp = path.with_extension("json.tmp");
        std::fs::write(
            &tmp,
            serde_json::to_vec_pretty(&shape).map_err(std::io::Error::other)?,
        )?;
        std::fs::rename(&tmp, path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tabs(open: &[i64], active: Option<i64>) -> ProjectTabs {
        ProjectTabs {
            open: open.to_vec(),
            active,
        }
    }

    #[test]
    fn opening_an_open_tab_only_activates_it() {
        let mut t = tabs(&[1, 2], Some(1));
        t.open_tab(2);
        assert_eq!((t.open(), t.active()), (&[1, 2][..], Some(2)));
        t.open_tab(3);
        assert_eq!((t.open(), t.active()), (&[1, 2, 3][..], Some(3)));
    }

    #[test]
    fn closing_the_active_tab_picks_the_left_neighbour_then_the_first_then_none() {
        let mut t = tabs(&[1, 2, 3], Some(2));
        t.close_tab(2);
        assert_eq!((t.open(), t.active()), (&[1, 3][..], Some(1)));
        t.close_tab(1);
        assert_eq!((t.open(), t.active()), (&[3][..], Some(3)));
        t.close_tab(3);
        assert_eq!((t.open(), t.active()), (&[][..], None));
    }

    #[test]
    fn closing_a_background_tab_keeps_the_active_one() {
        let mut t = tabs(&[1, 2, 3], Some(3));
        t.close_tab(1);
        assert_eq!(t.active(), Some(3));
        t.close_tab(99);
        assert_eq!(t.open(), &[2, 3]);
    }

    #[test]
    fn activating_a_tab_that_is_not_open_changes_nothing() {
        let mut t = tabs(&[1], Some(1));
        assert!(!t.activate(5));
        assert_eq!(t.active(), Some(1));
    }

    #[test]
    fn prune_drops_unregistered_projects_and_repairs_the_active_tab() {
        let mut t = tabs(&[1, 2, 3], Some(2));
        t.prune(&HashSet::from([1, 3]));
        assert_eq!((t.open(), t.active()), (&[1, 3][..], Some(1)));
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("project_tabs.json");
        let t = tabs(&[4, 7], Some(7));
        t.save(&path).unwrap();
        assert_eq!(
            ProjectTabs::load(&path),
            Loaded {
                tabs: t,
                save_blocked: false
            }
        );
        assert!(!path.with_extension("json.tmp").exists());
    }

    #[test]
    fn a_missing_or_garbage_file_loads_as_empty_and_may_be_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("project_tabs.json");
        assert_eq!(
            ProjectTabs::load(&path),
            Loaded {
                tabs: ProjectTabs::default(),
                save_blocked: false
            }
        );
        for junk in ["not json", "[1,2]", "42", "null", "\"x\""] {
            std::fs::write(&path, junk).unwrap();
            let loaded = ProjectTabs::load(&path);
            assert_eq!(
                loaded,
                Loaded {
                    tabs: ProjectTabs::default(),
                    save_blocked: false
                },
                "{junk}"
            );
        }
    }

    #[test]
    fn missing_fields_take_defaults_and_duplicates_and_a_dangling_active_are_repaired() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("project_tabs.json");
        std::fs::write(&path, r#"{"open":[3,3,5],"active":9}"#).unwrap();
        let loaded = ProjectTabs::load(&path);
        assert_eq!(
            (loaded.tabs.open(), loaded.tabs.active()),
            (&[3, 5][..], Some(3))
        );
        std::fs::write(&path, "{}").unwrap();
        assert_eq!(ProjectTabs::load(&path).tabs, ProjectTabs::default());
    }

    #[test]
    fn a_newer_schema_file_is_shown_empty_and_flagged_unsaveable() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("project_tabs.json");
        std::fs::write(&path, r#"{"schema":99,"open":[1],"active":1}"#).unwrap();
        assert_eq!(
            ProjectTabs::load(&path),
            Loaded {
                tabs: ProjectTabs::default(),
                save_blocked: true
            }
        );
    }

    #[test]
    fn moving_a_tab_keeps_the_others_in_order_and_the_active_one_unchanged() {
        let mut t = tabs(&[1, 2, 3], Some(2));
        assert!(t.move_tab(1, 2));
        assert_eq!((t.open(), t.active()), (&[2, 3, 1][..], Some(2)));
        assert!(t.move_tab(1, 0));
        assert_eq!(t.open(), &[1, 2, 3][..]);
    }

    #[test]
    fn moving_a_tab_to_where_it_already_is_is_a_no_op() {
        let mut t = tabs(&[1, 2, 3], Some(1));
        assert!(!t.move_tab(2, 1));
        assert_eq!(t.open(), &[1, 2, 3][..]);
        assert!(!t.move_tab(9, 0), "不在已开页签里:不动");
    }
}
