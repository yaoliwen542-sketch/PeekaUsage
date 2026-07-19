//! 原子文件写入与损坏文件备份工具
//!
//! 背景：config.json / keys.dat / usage_stats.json 之前都是 `fs::write` 直写目标
//! 文件，写入中途进程崩溃或断电会留下半截文件；下次启动解析失败后直接回退默认值，
//! 用户配置/凭据/统计历史全部静默丢失。
//!
//! 这里提供两个原语：
//! - [`atomic_write`]：先写同目录 `<文件名>.tmp`，再 rename 覆盖目标，
//!   保证任何时刻目标路径要么是老版本、要么是新版本，绝不出现半截文件。
//! - [`backup_corrupted_file`]：解析失败时把原文件改名成 `<文件名>.bak`，
//!   回退默认值的同时给用户留下手动恢复的可能。

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// 原子写入：写临时文件 -> flush 落盘 -> rename 覆盖目标。
///
/// Windows 的 `fs::rename` 不能覆盖已存在的目标文件（ERROR_ALREADY_EXISTS），
/// 所以 rename 失败时回退为"先删目标再 rename"。该回退路径存在极短的
/// "目标缺失"窗口，但临时文件已完整落盘，读取方在目标缺失时会回退默认值，
/// 之后任意一次成功保存都会重建目标文件，不会出现损坏的中间态。
pub fn atomic_write(path: &Path, content: &[u8]) -> io::Result<()> {
    let tmp_path = sibling_path(path, "tmp");

    // 清理上次可能残留的临时文件，避免 Windows 上 rename 冲突
    if tmp_path.exists() {
        let _ = fs::remove_file(&tmp_path);
    }

    // 写临时文件并尽量刷盘，缩小断电丢失窗口
    let mut file = fs::File::create(&tmp_path)?;
    file.write_all(content)?;
    file.sync_all()?;

    match fs::rename(&tmp_path, path) {
        Ok(()) => Ok(()),
        Err(_) => {
            // Windows：目标已存在时 rename 失败，先删目标再 rename
            fs::remove_file(path)?;
            fs::rename(&tmp_path, path)
        }
    }
}

/// 把损坏无法解析的文件备份为同目录 `<文件名>.bak`，便于用户手动恢复。
///
/// 使用 rename（移动）而不是 copy：原路径让位给后续重新生成的默认文件，
/// 避免每次启动都对同一个损坏文件重复备份。已存在的旧 .bak 会被覆盖
/// （保留最新一次损坏现场即可）。备份失败只打日志，不阻断启动。
pub fn backup_corrupted_file(path: &Path) {
    if !path.exists() {
        return;
    }

    let backup_path = sibling_path(path, "bak");
    if backup_path.exists() {
        let _ = fs::remove_file(&backup_path);
    }

    if let Err(error) = fs::rename(path, &backup_path) {
        eprintln!(
            "备份损坏文件失败 ({} -> {}): {}",
            path.display(),
            backup_path.display(),
            error
        );
    } else {
        eprintln!(
            "检测到损坏文件，已备份为 {} 并回退默认内容",
            backup_path.display()
        );
    }
}

/// 构造同目录兄弟路径：`config.json` + "tmp" -> `config.json.tmp`
fn sibling_path(path: &Path, extra_extension: &str) -> PathBuf {
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    path.with_file_name(format!("{}.{}", file_name, extra_extension))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sibling_path_appends_extension() {
        let path = Path::new("config.json");
        assert_eq!(sibling_path(path, "tmp"), Path::new("config.json.tmp"));
        assert_eq!(sibling_path(path, "bak"), Path::new("config.json.bak"));
    }

    #[test]
    fn test_atomic_write_creates_and_overwrites() {
        let dir = std::env::temp_dir().join(format!("peeka_atomic_test_{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let target = dir.join("config.json");

        atomic_write(&target, b"first").unwrap();
        assert_eq!(fs::read_to_string(&target).unwrap(), "first");

        // 覆盖已存在的目标（Windows 上 rename 需先删目标的回退路径）
        atomic_write(&target, b"second").unwrap();
        assert_eq!(fs::read_to_string(&target).unwrap(), "second");

        // 临时文件不应残留
        assert!(!sibling_path(&target, "tmp").exists());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_backup_corrupted_file_moves_original() {
        let dir = std::env::temp_dir().join(format!("peeka_backup_test_{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let target = dir.join("keys.dat");
        fs::write(&target, b"corrupted").unwrap();

        backup_corrupted_file(&target);
        assert!(!target.exists());
        assert_eq!(
            fs::read_to_string(sibling_path(&target, "bak")).unwrap(),
            "corrupted"
        );

        let _ = fs::remove_dir_all(&dir);
    }
}
