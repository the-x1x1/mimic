//! Copies `Mimic.lrplugin` into the app-managed location that Lightroom's
//! Plugin Manager is pointed at (spec §49). We never touch Lightroom's own
//! preferences or plugin folders.

use std::path::Path;

pub fn sync_plugin(src: &Path, dest: &Path) -> std::io::Result<usize> {
    std::fs::create_dir_all(dest)?;
    let mut copied = 0usize;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let name = entry.file_name();
        let from = entry.path();
        let to = dest.join(&name);
        if from.is_dir() {
            copied += sync_plugin(&from, &to)?;
            continue;
        }
        let needs_copy = match (std::fs::metadata(&from), std::fs::metadata(&to)) {
            (Ok(a), Ok(b)) => a.len() != b.len() || a.modified().ok() > b.modified().ok(),
            _ => true,
        };
        if needs_copy {
            std::fs::copy(&from, &to)?;
            copied += 1;
        }
    }
    Ok(copied)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copies_and_is_idempotent() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src");
        std::fs::create_dir_all(src.join("sub")).unwrap();
        std::fs::write(src.join("Info.lua"), "return {}").unwrap();
        std::fs::write(src.join("sub").join("x.lua"), "x").unwrap();
        let dest = tmp.path().join("dest");
        assert_eq!(sync_plugin(&src, &dest).unwrap(), 2);
        assert!(dest.join("sub").join("x.lua").is_file());
        assert_eq!(sync_plugin(&src, &dest).unwrap(), 0);
    }
}
