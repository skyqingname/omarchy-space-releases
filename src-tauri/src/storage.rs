use serde::{Deserialize, Serialize};
use std::{fs, path::{Path, PathBuf}};

const SETTINGS_FILE: &str = "storage-location.json";

#[derive(Deserialize, Serialize)]
struct Settings { directory: PathBuf }

pub struct Storage {
    default_directory: PathBuf,
    directory: PathBuf,
}

impl Storage {
    pub fn load(default_directory: PathBuf) -> Result<Self, String> {
        let settings = default_directory.join(SETTINGS_FILE);
        let directory = match fs::read(&settings) {
            Ok(bytes) => {
                let saved: Settings = serde_json::from_slice(&bytes).map_err(|_| "存放位置设置格式不正确，请检查 storage-location.json".to_string())?;
                if !saved.directory.is_absolute() { return Err("保存的存放位置不是绝对路径".into()); }
                saved.directory
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => default_directory.clone(),
            Err(error) => return Err(format!("无法读取存放位置设置：{error}")),
        };
        // Keep the original default spelling: it may be a user-managed junction.
        // A missing custom drive must not silently switch back to the default.
        Ok(Self { default_directory, directory })
    }

    pub fn directory(&self) -> &Path { &self.directory }
    pub fn default_directory(&self) -> &Path { &self.default_directory }

    pub fn can_change(&self) -> Result<bool, String> {
        Ok(!has_environment(&self.directory)?)
    }

    pub fn select(&mut self, requested: &Path) -> Result<(), String> {
        if !requested.is_absolute() { return Err("请选择完整的存放位置路径".into()); }
        let target = dunce::simplified(requested).to_path_buf();
        if !fs::metadata(&target).map_err(|error| format!("无法访问所选存放位置：{error}"))?.is_dir() {
            return Err("存放位置必须是文件夹".into());
        }
        let same = same_directory(&self.directory, &target)?;
        if !same {
            if has_environment(&self.directory)? {
                return Err("已有环境暂不支持更换存放位置；当前磁盘不会搬迁".into());
            }
            let vm = target.join("vm");
            if vm.try_exists().map_err(|error| error.to_string())? {
                let mut entries = fs::read_dir(&vm).map_err(|error| format!("无法检查目标文件夹：{error}"))?;
                if entries.next().transpose().map_err(|error| error.to_string())?.is_some() {
                    return Err("所选位置的 vm 文件夹已有内容，请选择空文件夹；不会覆盖或搬迁现有环境".into());
                }
            }
            if target.join("deployment.json").try_exists().map_err(|error| error.to_string())? {
                return Err("所选位置已有环境记录，请选择空文件夹".into());
            }
        }
        // These are product data directories, not temporary write probes.
        fs::create_dir_all(target.join("vm")).map_err(|error| format!("无法在所选位置创建虚拟机目录：{error}"))?;
        fs::create_dir_all(target.join("omarchy-image")).map_err(|error| format!("无法在所选位置创建镜像目录：{error}"))?;
        let pending = self.default_directory.join("storage-location.pending");
        let bytes = serde_json::to_vec_pretty(&Settings { directory: target.clone() }).map_err(|error| error.to_string())?;
        fs::write(&pending, bytes).map_err(|error| format!("无法保存存放位置：{error}"))?;
        fs::rename(pending, self.default_directory.join(SETTINGS_FILE)).map_err(|error| format!("无法保存存放位置：{error}"))?;
        self.directory = target;
        Ok(())
    }
}

fn has_environment(directory: &Path) -> Result<bool, String> {
    for name in ["vm/machine.json", "vm/disk.img"] {
        if directory.join(name).try_exists().map_err(|error| format!("无法检查现有环境：{error}"))? { return Ok(true); }
    }
    Ok(false)
}

fn same_directory(current: &Path, selected: &Path) -> Result<bool, String> {
    if current == selected { return Ok(true); }
    // Resolve links for identity only; never rewrite or remove the user's junction.
    let current = current.canonicalize().map_err(|error| format!("当前存放位置无法访问：{error}"))?;
    let selected = selected.canonicalize().map_err(|error| format!("所选存放位置无法访问：{error}"))?;
    Ok(current == selected)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> PathBuf {
        // Retain diagnostic fixtures until the user authorizes cleanup.
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../evidence/2026-11-04-storage-selection/test-fixtures").join(uuid::Uuid::new_v4().to_string());
        fs::create_dir_all(&root).unwrap();
        root.canonicalize().unwrap()
    }

    fn directory(root: &Path, name: &str) -> PathBuf {
        let path = root.join(name);
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn default_location_is_unchanged_without_a_preference() {
        let root = fixture();
        let original = directory(&root, "original");
        let storage = Storage::load(original.clone()).unwrap();
        assert_eq!(storage.directory(), original);
        assert!(!original.join(SETTINGS_FILE).exists());
    }

    #[test]
    fn selected_location_survives_restart_and_owns_vm_and_download_directories() {
        let root = fixture();
        let original = directory(&root, "original");
        let selected = directory(&root, "数据位置");
        let mut storage = Storage::load(original.clone()).unwrap();
        storage.select(&selected).unwrap();
        let reopened = Storage::load(original.clone()).unwrap();
        // Selection intentionally simplifies Windows verbatim path prefixes.
        assert_eq!(reopened.directory(), dunce::simplified(&selected));
        assert!(selected.join("vm").is_dir());
        assert!(selected.join("omarchy-image").is_dir());
        assert!(!original.join("vm").exists());
        assert!(original.join(SETTINGS_FILE).is_file());
        assert!(!selected.join(SETTINGS_FILE).exists());
        fs::write(selected.join("omarchy-image/retained.part"), b"existing download").unwrap();
        let next = directory(&root, "next-drive");
        storage.select(&next).unwrap();
        assert_eq!(Storage::load(original).unwrap().directory(), dunce::simplified(&next));
        assert_eq!(fs::read(selected.join("omarchy-image/retained.part")).unwrap(), b"existing download");
        assert!(!next.join("omarchy-image/retained.part").exists());
    }

    #[test]
    fn existing_environment_cannot_be_hidden_by_selecting_a_new_location() {
        for file in ["vm/disk.img", "vm/machine.json"] {
            let root = fixture();
            let original = directory(&root, "original");
            let selected = directory(&root, "selected");
            fs::create_dir_all(original.join("vm")).unwrap();
            fs::write(original.join(file), b"existing environment").unwrap();
            let mut storage = Storage::load(original.clone()).unwrap();
            assert!(!storage.can_change().unwrap());
            assert!(storage.select(&selected).unwrap_err().contains("暂不支持"));
            assert_eq!(storage.directory(), original);
            assert_eq!(fs::read(original.join(file)).unwrap(), b"existing environment");
            assert!(!selected.join("vm").exists());
            assert!(!original.join(SETTINGS_FILE).exists());
        }
    }

    #[test]
    fn target_vm_data_is_never_overwritten_or_implicitly_adopted() {
        let root = fixture();
        let original = directory(&root, "original");
        let selected = directory(&root, "selected");
        fs::create_dir_all(selected.join("vm")).unwrap();
        fs::write(selected.join("vm/disk.preparing"), b"keep target data").unwrap();
        let mut storage = Storage::load(original.clone()).unwrap();
        assert!(storage.select(&selected).unwrap_err().contains("已有内容"));
        assert_eq!(fs::read(selected.join("vm/disk.preparing")).unwrap(), b"keep target data");
        assert_eq!(storage.directory(), original);
    }

    #[test]
    fn failed_preference_write_leaves_the_active_location_unchanged() {
        let root = fixture();
        let original = directory(&root, "original");
        let selected = directory(&root, "selected");
        fs::create_dir(original.join("storage-location.pending")).unwrap();
        let mut storage = Storage::load(original.clone()).unwrap();
        assert!(storage.select(&selected).is_err());
        assert_eq!(storage.directory(), original);
        assert!(!original.join(SETTINGS_FILE).exists());
    }

    #[test]
    fn unavailable_custom_location_does_not_silently_fall_back() {
        let root = fixture();
        let original = directory(&root, "original");
        let missing = root.join("unmounted-data");
        fs::write(original.join(SETTINGS_FILE), serde_json::to_vec(&Settings { directory: missing.clone() }).unwrap()).unwrap();
        let storage = Storage::load(original).unwrap();
        assert_eq!(storage.directory(), missing);
        assert!(!missing.exists());
        assert!(crate::host::free_disk_gib(storage.directory()).is_err());
    }

    #[test]
    fn invalid_saved_location_is_reported_instead_of_reset() {
        let root = fixture();
        let original = directory(&root, "original");
        fs::write(original.join(SETTINGS_FILE), br#"{"directory":"relative-path"}"#).unwrap();
        assert!(Storage::load(original.clone()).is_err());
        fs::write(original.join(SETTINGS_FILE), b"broken json").unwrap();
        assert!(Storage::load(original).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn default_link_is_preserved_and_the_same_physical_location_is_not_a_migration() {
        let root = fixture();
        let target = directory(&root, "actual-drive");
        fs::create_dir(target.join("vm")).unwrap();
        fs::write(target.join("vm/disk.img"), b"existing linked disk").unwrap();
        let original = root.join("default-link");
        std::os::unix::fs::symlink(&target, &original).unwrap();
        let mut storage = Storage::load(original.clone()).unwrap();
        assert_eq!(storage.directory(), original);
        assert!(!storage.can_change().unwrap());
        storage.select(&target).unwrap();
        assert!(original.symlink_metadata().unwrap().file_type().is_symlink());
        assert_eq!(fs::read(target.join("vm/disk.img")).unwrap(), b"existing linked disk");
        assert_eq!(Storage::load(original).unwrap().directory(), target);
    }

    #[test]
    fn asset_pack_downloads_follow_selected_location_while_offline_images_stay_in_pack() {
        let root = fixture();
        let selected = directory(&root, "selected");
        let pack = directory(&root, "runtime-pack");
        for name in ["qemu", "zstd", "sing-box"] { fs::write(pack.join(name), b"fixture only").unwrap(); }
        let manifest = serde_json::json!({
            "platform":crate::config::platform(),"image":"factory.img.zst","image_format":"zstd","image_version":"fixture",
            "qemu":"qemu","zstd":"zstd","sing_box_package":"sing-box","efi_variables_bytes":0,
            "arguments":[],"network_device_id":"nic","downloads":{"factory.img.zst":[{"url":"https://example.invalid/image","bytes":4}]}
        });
        fs::write(pack.join("manifest.json"), serde_json::to_vec(&manifest).unwrap()).unwrap();
        let online = crate::config::AssetPack::load(&pack, &selected).unwrap();
        assert_eq!(online.images, selected.join("omarchy-image"));
        assert!(!online.image_ready());
        fs::write(pack.join("factory.img.zst"), b"test").unwrap();
        let offline = crate::config::AssetPack::load(&pack, &selected).unwrap();
        assert_eq!(offline.images, pack);
        assert!(offline.image_ready());
    }
}
