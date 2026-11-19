//! Makes the bundled `clab` runnable from a terminal on macOS, where a DMG has no install step.
//! The link goes in `~/.local/bin`, so no admin rights are needed.

use std::{io, os::unix::fs::symlink, path::Path};

/// Points `<bin_dir>/clab` at `target`. The app may have moved since the last start, so a
/// link into an older ChassisLab bundle is repointed. Anything else called `clab` is the
/// user's and stays untouched.
pub fn link(target: &Path, bin_dir: &Path) -> io::Result<()> {
    let link = bin_dir.join("clab");
    match std::fs::read_link(&link) {
        Ok(current) if current == target => return Ok(()),
        Ok(current) if current.ends_with("clab-runtime/bin/clab") => std::fs::remove_file(&link)?,
        Ok(_) => return Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        // Not a link: a file the user put there.
        Err(_) => return Ok(()),
    }
    std::fs::create_dir_all(bin_dir)?;
    symlink(target, link)
}

/// Does nothing in development builds, which have no bundled runtime.
#[cfg(target_os = "macos")]
pub fn install(app: &tauri::AppHandle) -> Result<(), String> {
    use tauri::Manager;

    let target = app
        .path()
        .resource_dir()
        .map_err(|error| error.to_string())?
        .join("clab-runtime/bin/clab");
    if !target.is_file() {
        return Ok(());
    }
    let home = std::env::var_os("HOME").ok_or("HOME is not set")?;
    link(&target, &Path::new(&home).join(".local/bin")).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const OLD: &str = "/Old/ChassisLab.app/Contents/Resources/clab-runtime/bin/clab";
    const NEW: &str = "/New/ChassisLab.app/Contents/Resources/clab-runtime/bin/clab";

    fn target(dir: &Path) -> std::path::PathBuf {
        dir.join("bundle/clab-runtime/bin/clab")
    }

    #[test]
    fn creates_the_folder_and_the_link() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join(".local/bin");
        link(Path::new(NEW), &bin).unwrap();
        assert_eq!(
            std::fs::read_link(bin.join("clab")).unwrap(),
            Path::new(NEW)
        );
        link(Path::new(NEW), &bin).unwrap();
    }

    #[test]
    fn repoints_a_link_into_an_older_bundle() {
        let dir = tempfile::tempdir().unwrap();
        symlink(OLD, dir.path().join("clab")).unwrap();
        link(Path::new(NEW), dir.path()).unwrap();
        assert_eq!(
            std::fs::read_link(dir.path().join("clab")).unwrap(),
            Path::new(NEW)
        );
    }

    #[test]
    fn leaves_other_links_and_files_alone() {
        let dir = tempfile::tempdir().unwrap();
        symlink("/opt/other/clab", dir.path().join("clab")).unwrap();
        link(&target(dir.path()), dir.path()).unwrap();
        assert_eq!(
            std::fs::read_link(dir.path().join("clab")).unwrap(),
            Path::new("/opt/other/clab")
        );

        let other = tempfile::tempdir().unwrap();
        std::fs::write(other.path().join("clab"), "mine").unwrap();
        link(&target(other.path()), other.path()).unwrap();
        assert_eq!(
            std::fs::read_to_string(other.path().join("clab")).unwrap(),
            "mine"
        );
    }
}
