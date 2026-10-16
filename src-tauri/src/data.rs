use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    fs,
    io::Write,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

pub const DEFAULT_CATEGORY: &str = "默认";

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Prompt {
    pub id: String,
    pub title: String,
    pub content: String,
    pub category: String,
    pub tags: Vec<String>,
    pub favorite: bool,
    pub created_at: u64,
    pub updated_at: u64,
    pub last_used_at: Option<u64>,
    pub use_count: u64,
}
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub default_action: Action,
    pub send_key: SendKey,
    pub theme: Theme,
    pub shortcut: String,
}
#[derive(Clone, Copy, Serialize, Deserialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum Action {
    Send,
    Insert,
    Copy,
}
#[derive(Clone, Copy, Serialize, Deserialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum SendKey {
    Enter,
    CtrlEnter,
}
#[derive(Clone, Copy, Serialize, Deserialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum Theme {
    System,
    Light,
    Dark,
}
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct AppData {
    pub version: u32,
    #[serde(default)]
    pub categories: Vec<String>,
    pub prompts: Vec<Prompt>,
    pub settings: Settings,
}
#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "camelCase")]
pub enum Mutation {
    Upsert { prompt: Prompt },
    Delete { id: String },
    Favorite { id: String },
    Reorder { ids: Vec<String> },
    Settings { settings: Settings },
    Import { data: AppData, mode: String },
    CategoryCreate { name: String },
    CategoryDelete { name: String },
}
fn category_name(value: &str) -> Result<String, String> {
    let name = value.trim();
    if name.is_empty() || name.encode_utf16().count() > 40 {
        return Err("类别名称不能为空，且不能超过 40 个字符。".into());
    }
    Ok(name.to_owned())
}
pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
pub fn validate(data: &AppData) -> Result<(), String> {
    if data.version != 1 || data.prompts.len() > 2000 {
        return Err("提示词文件版本不正确或超过 2000 条。".into());
    }
    let categories: HashSet<_> = data.categories.iter().collect();
    if data.categories.len() > 2001
        || categories.len() != data.categories.len()
        || !data.categories.iter().any(|name| name == DEFAULT_CATEGORY)
        || data
            .categories
            .iter()
            .any(|name| category_name(name).as_ref() != Ok(name))
    {
        return Err("类别列表无效、重复或超过 2000 个自定义类别。".into());
    }
    let mut ids = HashSet::new();
    for p in &data.prompts {
        if p.id.is_empty() || p.id.len() > 240 || !ids.insert(&p.id) {
            return Err("提示词 ID 为空、重复或过长。".into());
        }
        if p.title.trim().is_empty()
            || p.title.chars().count() > 100
            || p.content.trim().is_empty()
            || p.content.chars().count() > 100_000
            || p.category.chars().count() > 40
            || !categories.contains(&p.category)
            || p.tags.len() > 20
            || p.tags.iter().any(|t| t.chars().count() > 40)
        {
            return Err(format!("「{}」的内容格式不正确。", p.title));
        }
    }
    if data.settings.shortcut.trim().is_empty() || data.settings.shortcut.len() > 100 {
        return Err("快捷键格式不正确。".into());
    }
    Ok(())
}
impl AppData {
    pub fn normalize_categories(&mut self) -> Result<(), String> {
        let mut categories = vec![DEFAULT_CATEGORY.to_owned()];
        for name in &self.categories {
            let name = category_name(name)?;
            if !categories.contains(&name) {
                categories.push(name);
            }
        }
        for prompt in &mut self.prompts {
            prompt.category = if prompt.category.trim().is_empty() {
                DEFAULT_CATEGORY.into()
            } else {
                category_name(&prompt.category)?
            };
            if !categories.contains(&prompt.category) {
                categories.push(prompt.category.clone());
            }
        }
        self.categories = categories;
        Ok(())
    }
    fn migrate_starter_content(&mut self) {
        let legacy: Self = serde_json::from_str(include_str!("../legacy-default-data.json"))
            .expect("bundled legacy data is valid");
        let previous_count = self.prompts.len();
        self.prompts.retain(|prompt| {
            !legacy.prompts.iter().any(|old| {
                prompt.id == old.id
                    && prompt.title == old.title
                    && prompt.content == old.content
                    && prompt.category == old.category
                    && prompt.tags == old.tags
            })
        });
        if previous_count != self.prompts.len()
            && !self
                .prompts
                .iter()
                .any(|prompt| prompt.id == "starter-test")
        {
            self.prompts.push(Self::seed().prompts.remove(0));
        }
    }
    pub fn seed() -> Self {
        let mut data: Self = serde_json::from_str(include_str!("../default-data.json"))
            .expect("bundled seed data is valid");
        let timestamp = now();
        for p in &mut data.prompts {
            p.created_at = timestamp;
            p.updated_at = timestamp;
        }
        data
    }
    pub fn apply(&mut self, action: Mutation) -> Result<(), String> {
        match action {
            Mutation::Upsert { mut prompt } => {
                prompt.title = prompt.title.trim().to_owned();
                prompt.category = prompt.category.trim().to_owned();
                if prompt.category.is_empty() {
                    prompt.category = DEFAULT_CATEGORY.into();
                }
                if !self.categories.contains(&prompt.category) {
                    return Err("所选类别已不存在，请重新选择类别。".into());
                }
                if let Some(old) = self.prompts.iter_mut().find(|p| p.id == prompt.id) {
                    prompt.last_used_at = old.last_used_at;
                    prompt.use_count = old.use_count;
                    *old = prompt;
                } else {
                    self.prompts.insert(0, prompt);
                }
            }
            Mutation::Delete { id } => {
                self.prompts.retain(|p| p.id != id);
            }
            Mutation::Favorite { id } => {
                let item = self
                    .prompts
                    .iter_mut()
                    .find(|p| p.id == id)
                    .ok_or("提示词已不存在，请刷新。")?;
                item.favorite = !item.favorite;
                item.updated_at = now();
            }
            Mutation::Reorder { ids } => {
                let set: HashSet<_> = ids.iter().collect();
                if ids.len() != self.prompts.len()
                    || set.len() != ids.len()
                    || self.prompts.iter().any(|p| !set.contains(&p.id))
                {
                    return Err("提示词库已更新，请重新排序。".into());
                }
                self.prompts
                    .sort_by_key(|p| ids.iter().position(|id| *id == p.id).unwrap_or(usize::MAX));
            }
            Mutation::Settings { settings } => self.settings = settings,
            Mutation::CategoryCreate { name } => {
                let name = category_name(&name)?;
                if self.categories.contains(&name) {
                    return Err("这个类别已存在。".into());
                }
                self.categories.push(name);
            }
            Mutation::CategoryDelete { name } => {
                let name = category_name(&name)?;
                if name == DEFAULT_CATEGORY {
                    return Err("默认类别不能删除。".into());
                }
                if !self.categories.contains(&name) {
                    return Err("类别已不存在，请刷新。".into());
                }
                self.categories.retain(|category| *category != name);
                for prompt in &mut self.prompts {
                    if prompt.category == name {
                        prompt.category = DEFAULT_CATEGORY.into();
                        prompt.updated_at = now();
                    }
                }
            }
            Mutation::Import { mut data, mode } => {
                data.normalize_categories()?;
                validate(&data)?;
                match mode.as_str() {
                    "replace" => *self = data,
                    "merge" => {
                        for category in data.categories {
                            if !self.categories.contains(&category) {
                                self.categories.push(category);
                            }
                        }
                        for prompt in data.prompts {
                            if let Some(old) = self.prompts.iter_mut().find(|p| p.id == prompt.id) {
                                if old.updated_at < prompt.updated_at {
                                    *old = prompt;
                                }
                            } else {
                                self.prompts.push(prompt);
                            }
                        }
                    }
                    _ => return Err("不支持的导入方式。".into()),
                }
            }
        }
        validate(self)
    }
    pub fn record_use(&mut self, id: &str) {
        if let Some(p) = self.prompts.iter_mut().find(|p| p.id == id) {
            p.last_used_at = Some(now());
            p.use_count = p.use_count.saturating_add(1);
        }
    }
}
pub fn read_data(path: &Path) -> Result<AppData, String> {
    if !path.exists() {
        return Ok(AppData::seed());
    }
    let meta = fs::metadata(path).map_err(|e| e.to_string())?;
    if meta.len() > 10 * 1024 * 1024 {
        return Err("提示词数据文件超过 10 MB。".into());
    }
    let raw = fs::read_to_string(path).map_err(|e| format!("无法读取提示词库：{e}"))?;
    let value: serde_json::Value = serde_json::from_str(&raw).map_err(|e| {
        format!("提示词库格式损坏；原文件已保留。可检查同目录中的 .backup.json 备份。详情：{e}")
    })?;
    let legacy_schema = value.get("categories").is_none();
    let mut data: AppData = serde_json::from_value(value)
        .map_err(|e| format!("提示词库格式不正确；原文件已保留：{e}"))?;
    let original = data.clone();
    data.normalize_categories()?;
    validate(&data)?;
    if legacy_schema {
        data.migrate_starter_content();
        data.categories.clear();
        data.normalize_categories()?;
        validate(&data)?;
    }
    if legacy_schema || data != original {
        write_data(path, &data)?;
    }
    Ok(data)
}
pub fn write_data(path: &Path, data: &AppData) -> Result<(), String> {
    validate(data)?;
    let parent = path.parent().ok_or("数据目录无效。")?;
    fs::create_dir_all(parent).map_err(|e| format!("无法创建数据目录：{e}"))?;
    let bytes = serde_json::to_vec_pretty(data).map_err(|e| e.to_string())?;
    let temp = path.with_extension("pending.json");
    {
        let mut file = fs::File::create(&temp).map_err(|e| e.to_string())?;
        file.write_all(&bytes)
            .and_then(|_| file.sync_all())
            .map_err(|e| format!("保存数据失败：{e}"))?;
    }
    if path.exists() {
        fs::copy(path, path.with_extension("backup.json"))
            .map_err(|e| format!("保存备份失败：{e}"))?;
    }
    // Windows MoveFileEx(REPLACE_EXISTING | WRITE_THROUGH) atomically replaces an existing file.
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows::{
            core::PCWSTR,
            Win32::Storage::FileSystem::{
                MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
            },
        };
        let source: Vec<u16> = temp.as_os_str().encode_wide().chain(Some(0)).collect();
        let target: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        unsafe {
            MoveFileExW(
                PCWSTR(source.as_ptr()),
                PCWSTR(target.as_ptr()),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
            .map_err(|e| format!("无法替换数据文件：{e}"))?;
        }
    }
    #[cfg(not(windows))]
    fs::rename(&temp, path).map_err(|e| e.to_string())?;
    Ok(())
}
pub fn data_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    use tauri::Manager;
    if let Some(folder) = std::env::var_os("PROMPTDOCK_DATA_DIR") {
        return Ok(PathBuf::from(folder).join("prompts.json"));
    }
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("prompts.json"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn seed_contains_only_default_and_test() {
        let data = AppData::seed();
        assert_eq!(data.categories, vec![DEFAULT_CATEGORY]);
        assert_eq!(data.prompts.len(), 1);
        assert_eq!(data.prompts[0].title, "test");
        assert_eq!(data.prompts[0].content, "test");
        validate(&data).unwrap();
    }
    #[test]
    fn empty_categories_survive_serialization_and_last_prompt_deletion() {
        let mut data = AppData::seed();
        data.apply(Mutation::CategoryCreate {
            name: "空类别".into(),
        })
        .unwrap();
        data.apply(Mutation::Delete {
            id: "starter-test".into(),
        })
        .unwrap();
        let restored: AppData =
            serde_json::from_str(&serde_json::to_string(&data).unwrap()).unwrap();
        assert_eq!(restored.categories, vec![DEFAULT_CATEGORY, "空类别"]);
        assert!(restored.prompts.is_empty());
    }
    #[test]
    fn deleting_category_moves_prompts_and_keeps_usage() {
        let mut data = AppData::seed();
        data.apply(Mutation::CategoryCreate {
            name: "工作".into(),
        })
        .unwrap();
        data.prompts[0].category = "工作".into();
        data.prompts[0].use_count = 7;
        data.apply(Mutation::CategoryDelete {
            name: "工作".into(),
        })
        .unwrap();
        assert_eq!(data.categories, vec![DEFAULT_CATEGORY]);
        assert_eq!(data.prompts[0].category, DEFAULT_CATEGORY);
        assert_eq!(data.prompts[0].content, "test");
        assert_eq!(data.prompts[0].use_count, 7);
    }
    #[test]
    fn default_duplicate_and_invalid_categories_are_rejected() {
        let mut data = AppData::seed();
        for name in [DEFAULT_CATEGORY, " "] {
            assert!(data
                .apply(Mutation::CategoryCreate { name: name.into() })
                .is_err());
        }
        assert!(data
            .apply(Mutation::CategoryCreate {
                name: "x".repeat(41)
            })
            .is_err());
        assert!(data
            .apply(Mutation::CategoryDelete {
                name: DEFAULT_CATEGORY.into()
            })
            .is_err());
    }
    #[test]
    fn stale_prompt_cannot_recreate_deleted_category() {
        let mut data = AppData::seed();
        data.apply(Mutation::CategoryCreate {
            name: "工作".into(),
        })
        .unwrap();
        data.prompts[0].category = "工作".into();
        let stale = data.prompts[0].clone();
        data.apply(Mutation::CategoryDelete {
            name: "工作".into(),
        })
        .unwrap();
        assert!(data.apply(Mutation::Upsert { prompt: stale }).is_err());
        assert_eq!(data.categories, vec![DEFAULT_CATEGORY]);
    }
    #[test]
    fn imports_keep_empty_categories_and_old_backups_are_not_pruned() {
        let mut data = AppData::seed();
        data.apply(Mutation::CategoryCreate {
            name: "本地空类别".into(),
        })
        .unwrap();
        let mut incoming = AppData::seed();
        incoming.categories.push("导入空类别".into());
        data.apply(Mutation::Import {
            data: incoming.clone(),
            mode: "merge".into(),
        })
        .unwrap();
        assert!(data.categories.contains(&"本地空类别".into()));
        assert!(data.categories.contains(&"导入空类别".into()));
        data.apply(Mutation::Import {
            data: incoming,
            mode: "replace".into(),
        })
        .unwrap();
        assert_eq!(data.categories, vec![DEFAULT_CATEGORY, "导入空类别"]);
        let legacy: AppData =
            serde_json::from_str(include_str!("../legacy-default-data.json")).unwrap();
        data.apply(Mutation::Import {
            data: legacy,
            mode: "replace".into(),
        })
        .unwrap();
        assert_eq!(data.prompts.len(), 9);
        assert!(data.categories.contains(&"编程".into()));
    }
    struct StoredFixture {
        folder: PathBuf,
    }
    impl StoredFixture {
        fn new() -> Self {
            let suffix = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let folder = std::env::current_dir()
                .unwrap()
                .join(".test-data")
                .join(format!("{}-{suffix}", std::process::id()));
            fs::create_dir_all(&folder).unwrap();
            Self { folder }
        }
        fn path(&self) -> PathBuf {
            self.folder.join("prompts.json")
        }
    }
    impl Drop for StoredFixture {
        fn drop(&mut self) {
            for name in [
                "prompts.json",
                "prompts.backup.json",
                "prompts.pending.json",
            ] {
                let _ = fs::remove_file(self.folder.join(name));
            }
            let _ = fs::remove_dir(&self.folder);
        }
    }
    #[test]
    fn storage_upgrade_keeps_user_edits_and_preserves_original_backup() {
        let fixture = StoredFixture::new();
        let mut legacy: serde_json::Value =
            serde_json::from_str(include_str!("../legacy-default-data.json")).unwrap();
        legacy["prompts"][0]["title"] = "我修改过的标题".into();
        let raw = serde_json::to_string(&legacy).unwrap();
        fs::write(fixture.path(), &raw).unwrap();
        let migrated = read_data(&fixture.path()).unwrap();
        assert_eq!(migrated.prompts.len(), 2);
        assert_eq!(migrated.prompts[0].title, "我修改过的标题");
        assert_eq!(migrated.prompts[1].title, "test");
        assert_eq!(migrated.categories, vec![DEFAULT_CATEGORY, "编程"]);
        let backup = fixture.path().with_extension("backup.json");
        assert_eq!(fs::read_to_string(&backup).unwrap(), raw);
        assert_eq!(read_data(&fixture.path()).unwrap(), migrated);
        assert_eq!(fs::read_to_string(backup).unwrap(), raw);
    }
    #[test]
    fn invalid_old_storage_is_not_cleaned_or_overwritten() {
        let fixture = StoredFixture::new();
        let mut legacy: serde_json::Value =
            serde_json::from_str(include_str!("../legacy-default-data.json")).unwrap();
        let duplicate = legacy["prompts"][0].clone();
        legacy["prompts"].as_array_mut().unwrap().push(duplicate);
        let raw = serde_json::to_string(&legacy).unwrap();
        fs::write(fixture.path(), &raw).unwrap();
        assert!(read_data(&fixture.path()).is_err());
        assert_eq!(fs::read_to_string(fixture.path()).unwrap(), raw);
        assert!(!fixture.path().with_extension("backup.json").exists());
    }
    #[test]
    fn rejects_duplicate_ids() {
        let mut data = AppData::seed();
        data.prompts.push(data.prompts[0].clone());
        assert!(validate(&data).is_err());
    }
    #[test]
    fn partial_reorder_never_drops_prompts() {
        let mut data = AppData::seed();
        let mut extra = data.prompts[0].clone();
        extra.id = "second".into();
        data.prompts.push(extra);
        let count = data.prompts.len();
        assert!(data
            .apply(Mutation::Reorder {
                ids: vec![data.prompts[0].id.clone()]
            })
            .is_err());
        assert_eq!(data.prompts.len(), count);
    }
    #[test]
    fn edits_preserve_newer_usage() {
        let mut data = AppData::seed();
        let stale = data.prompts[0].clone();
        data.record_use(&stale.id);
        data.apply(Mutation::Upsert { prompt: stale }).unwrap();
        assert_eq!(data.prompts[0].use_count, 1);
        assert!(data.prompts[0].last_used_at.is_some());
    }
    #[test]
    fn unknown_import_mode_is_rejected() {
        let mut data = AppData::seed();
        let copy = data.clone();
        assert!(data
            .apply(Mutation::Import {
                data: copy,
                mode: "invalid".into()
            })
            .is_err());
    }
}
