use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

pub const TRANSACTION_DIR: &str = ".super-instruct-transaction";

#[derive(Clone, Debug, Serialize, Deserialize)]
struct SnapshotEntry {
    relative_path: String,
    kind: SnapshotKind,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum SnapshotKind {
    Missing,
    File,
    Directory,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct TransactionJournal {
    schema_version: u32,
    transaction_id: String,
    operation: String,
    created_at: String,
    phase: String,
    entries: Vec<SnapshotEntry>,
}

pub struct FileTransaction {
    root: PathBuf,
    journal: TransactionJournal,
    finished: bool,
}

impl FileTransaction {
    pub fn begin(root: &Path, operation: &str, relative_paths: &[PathBuf]) -> Result<Self, String> {
        let live_dir = root.join(TRANSACTION_DIR);
        if live_dir.exists() {
            return Err(format!(
                "检测到未完成事务，请先恢复: {}",
                live_dir.display()
            ));
        }

        let transaction_id = uuid::Uuid::new_v4().simple().to_string();
        let staging = root.join(format!("{}.{}.pending", TRANSACTION_DIR, transaction_id));
        fs::create_dir(&staging)
            .map_err(|e| format!("create transaction directory failed: {e}"))?;
        let snapshots = staging.join("snapshots");
        fs::create_dir(&snapshots).map_err(|e| format!("create snapshot directory failed: {e}"))?;

        let result = (|| {
            let mut entries = Vec::new();
            for relative in unique_paths(relative_paths) {
                validate_relative_path(&relative)?;
                let source = root.join(&relative);
                let destination = snapshots.join(&relative);
                let kind = if source.is_file() {
                    if let Some(parent) = destination.parent() {
                        fs::create_dir_all(parent)
                            .map_err(|e| format!("create snapshot parent failed: {e}"))?;
                    }
                    fs::copy(&source, &destination)
                        .map_err(|e| format!("snapshot {} failed: {e}", source.display()))?;
                    SnapshotKind::File
                } else if source.is_dir() {
                    copy_dir_recursive(&source, &destination)
                        .map_err(|e| format!("snapshot {} failed: {e}", source.display()))?;
                    SnapshotKind::Directory
                } else if source.exists() {
                    return Err(format!(
                        "unsupported transaction path: {}",
                        source.display()
                    ));
                } else {
                    SnapshotKind::Missing
                };
                entries.push(SnapshotEntry {
                    relative_path: relative.to_string_lossy().into_owned(),
                    kind,
                });
            }

            let journal = TransactionJournal {
                schema_version: 1,
                transaction_id,
                operation: operation.to_string(),
                created_at: chrono::Utc::now().to_rfc3339(),
                phase: "prepared".to_string(),
                entries,
            };
            write_journal(&staging, &journal)?;
            fs::rename(&staging, &live_dir)
                .map_err(|e| format!("publish transaction journal failed: {e}"))?;
            Ok(journal)
        })();

        match result {
            Ok(journal) => Ok(Self {
                root: root.to_path_buf(),
                journal,
                finished: false,
            }),
            Err(error) => {
                let _ = fs::remove_dir_all(&staging);
                Err(error)
            }
        }
    }

    pub fn commit(mut self) -> Result<(), String> {
        let dir = self.root.join(TRANSACTION_DIR);
        fs::remove_dir_all(&dir).map_err(|e| format!("cleanup transaction journal failed: {e}"))?;
        self.finished = true;
        Ok(())
    }

    pub fn rollback(mut self) -> Result<(), String> {
        restore_from_journal(&self.root, &self.journal)?;
        let dir = self.root.join(TRANSACTION_DIR);
        fs::remove_dir_all(&dir)
            .map_err(|e| format!("cleanup rolled back transaction failed: {e}"))?;
        self.finished = true;
        Ok(())
    }
}

impl Drop for FileTransaction {
    fn drop(&mut self) {
        if !self.finished {
            tracing::warn!(
                "transaction {} left for startup recovery",
                self.journal.transaction_id
            );
        }
    }
}

pub fn has_pending(root: &Path) -> bool {
    root.join(TRANSACTION_DIR).is_dir()
}

pub fn recover_pending(root: &Path) -> Result<Option<String>, String> {
    let dir = root.join(TRANSACTION_DIR);
    if !dir.is_dir() {
        return Ok(None);
    }
    let content = fs::read_to_string(dir.join("journal.json"))
        .map_err(|e| format!("read transaction journal failed: {e}"))?;
    let journal: TransactionJournal =
        serde_json::from_str(&content).map_err(|e| format!("invalid transaction journal: {e}"))?;
    if journal.schema_version != 1 {
        return Err(format!(
            "unsupported transaction schema: {}",
            journal.schema_version
        ));
    }
    restore_from_journal(root, &journal)?;
    fs::remove_dir_all(&dir).map_err(|e| format!("cleanup recovered transaction failed: {e}"))?;
    Ok(Some(format!(
        "已恢复中断的 {} 事务 {}",
        journal.operation, journal.transaction_id
    )))
}

fn restore_from_journal(root: &Path, journal: &TransactionJournal) -> Result<(), String> {
    let snapshots = root.join(TRANSACTION_DIR).join("snapshots");
    for entry in journal.entries.iter().rev() {
        let relative = PathBuf::from(&entry.relative_path);
        validate_relative_path(&relative)?;
        let live = root.join(&relative);
        let snapshot = snapshots.join(&relative);
        remove_path(&live)?;
        match entry.kind {
            SnapshotKind::Missing => {}
            SnapshotKind::File => {
                if let Some(parent) = live.parent() {
                    fs::create_dir_all(parent)
                        .map_err(|e| format!("create restore parent failed: {e}"))?;
                }
                fs::copy(&snapshot, &live)
                    .map_err(|e| format!("restore {} failed: {e}", live.display()))?;
            }
            SnapshotKind::Directory => {
                copy_dir_recursive(&snapshot, &live)
                    .map_err(|e| format!("restore {} failed: {e}", live.display()))?;
            }
        }
    }
    Ok(())
}

fn write_journal(dir: &Path, journal: &TransactionJournal) -> Result<(), String> {
    let pending = dir.join("journal.pending.json");
    let live = dir.join("journal.json");
    let payload = serde_json::to_vec_pretty(journal)
        .map_err(|e| format!("serialize transaction journal failed: {e}"))?;
    fs::write(&pending, payload).map_err(|e| format!("write journal failed: {e}"))?;
    fs::rename(&pending, &live).map_err(|e| format!("publish journal failed: {e}"))?;
    Ok(())
}

fn unique_paths(paths: &[PathBuf]) -> Vec<PathBuf> {
    let mut values = paths.to_vec();
    values.sort();
    values.dedup();
    values
}

fn validate_relative_path(path: &Path) -> Result<(), String> {
    if path.is_absolute()
        || path
            .components()
            .any(|part| matches!(part, std::path::Component::ParentDir))
    {
        return Err(format!("unsafe transaction path: {}", path.display()));
    }
    Ok(())
}

fn remove_path(path: &Path) -> Result<(), String> {
    if path.is_dir() {
        fs::remove_dir_all(path).map_err(|e| format!("remove {} failed: {e}", path.display()))
    } else if path.exists() {
        fs::remove_file(path).map_err(|e| format!("remove {} failed: {e}", path.display()))
    } else {
        Ok(())
    }
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let path = entry.path();
        let destination = dst.join(entry.file_name());
        if path.is_dir() {
            copy_dir_recursive(&path, &destination)?;
        } else if path.is_file() {
            fs::copy(&path, &destination)?;
        } else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("unsupported snapshot node: {}", path.display()),
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transaction_rolls_back_files_and_directories() {
        let root = std::env::temp_dir().join(format!(
            "super-instruct-transaction-test-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(root.join("skills/demo")).unwrap();
        fs::write(root.join("config.toml"), "before").unwrap();
        fs::write(root.join("skills/demo/SKILL.md"), "before skill").unwrap();

        let transaction = FileTransaction::begin(
            &root,
            "test",
            &[PathBuf::from("config.toml"), PathBuf::from("skills/demo")],
        )
        .unwrap();
        fs::write(root.join("config.toml"), "after").unwrap();
        fs::remove_dir_all(root.join("skills/demo")).unwrap();
        transaction.rollback().unwrap();

        assert_eq!(
            fs::read_to_string(root.join("config.toml")).unwrap(),
            "before"
        );
        assert_eq!(
            fs::read_to_string(root.join("skills/demo/SKILL.md")).unwrap(),
            "before skill"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn pending_transaction_is_recovered_on_restart() {
        let root = std::env::temp_dir().join(format!(
            "super-instruct-transaction-recovery-test-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("config.toml"), "before").unwrap();

        let transaction =
            FileTransaction::begin(&root, "deploy", &[PathBuf::from("config.toml")]).unwrap();
        fs::write(root.join("config.toml"), "partial").unwrap();
        std::mem::forget(transaction);

        assert!(has_pending(&root));
        let message = recover_pending(&root).unwrap().unwrap();
        assert!(message.contains("deploy"));
        assert_eq!(
            fs::read_to_string(root.join("config.toml")).unwrap(),
            "before"
        );
        assert!(!has_pending(&root));
        fs::remove_dir_all(root).unwrap();
    }
}
