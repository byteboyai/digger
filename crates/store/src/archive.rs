//! Same-directory no-clobber archive. Linking reserves the destination atomically; unlinking
//! the source can be retried after a crash. Never use exists()+rename(), which can overwrite.
use std::{io, path::Path};
pub(crate) fn legacy(source: &Path) -> Result<(), String> {
    let mut name = source.as_os_str().to_owned();
    name.push(".migrated");
    let target = std::path::PathBuf::from(name);
    match std::fs::hard_link(source, &target) {
        Ok(()) => {}
        Err(error)
            if error.kind() == io::ErrorKind::AlreadyExists && same_file(source, &target) => {}
        Err(error) => return Err(format!("归档目标未覆盖 ({}): {error}", target.display())),
    }
    match std::fs::remove_file(source) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("归档已保留，移除旧名称失败: {error}")),
    }
}
#[cfg(unix)]
fn same_file(a: &Path, b: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (std::fs::metadata(a), std::fs::metadata(b)) {
        (Ok(a), Ok(b)) => a.dev() == b.dev() && a.ino() == b.ino(),
        _ => false,
    }
}
#[cfg(not(unix))]
fn same_file(_: &Path, _: &Path) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn an_existing_archive_is_never_overwritten_and_an_interrupted_link_can_resume() {
        let dir = std::env::temp_dir().join(format!("digger-archive-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let source = dir.join("source");
        let target = dir.join("source.migrated");
        std::fs::write(&source, "new").unwrap();
        std::fs::write(&target, "old").unwrap();
        assert!(legacy(&source).is_err());
        assert_eq!(std::fs::read(&target).unwrap(), b"old");
        assert_eq!(std::fs::read(&source).unwrap(), b"new");
        std::fs::remove_file(&target).unwrap();
        #[cfg(unix)]
        {
            std::fs::hard_link(&source, &target).unwrap(); // crash after publishing archive
            legacy(&source).unwrap();
        }
        #[cfg(not(unix))]
        legacy(&source).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"new");
        assert!(!source.exists());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
