//! `.clab` session files: a zip archive holding `session.json` and, when the
//! session embeds its inputs, copies of them under `inputs/`.
//!
//! A session file is untrusted input: unknown fields, other versions and
//! unexpected archive entries are rejected.

use std::{
    collections::hash_map::DefaultHasher,
    error::Error,
    fs::{self, File},
    hash::{Hash, Hasher},
    io::{self, Read},
    path::{Component, Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

type Result<T> = std::result::Result<T, String>;

const VERSION: u32 = 1;
const MANIFEST: &str = "session.json";
const INPUTS: &str = "inputs";
const MAX_ENTRIES: usize = 8;
const MAX_MANIFEST_BYTES: u64 = 1 << 20;
const MAX_EXTRACTED_BYTES: u64 = 64 << 30;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Session {
    version: u32,
    /// Whether input files are copied into the archive on save.
    embed: bool,
    convert: ConvertState,
    inspect: InspectState,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ConvertState {
    input: Option<PathBuf>,
    domain: String,
    test: Option<String>,
    output: Option<PathBuf>,
    force: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct InspectState {
    file: Option<PathBuf>,
    curves: Vec<Curve>,
    /// Whether the chart was drawn; absent in sessions saved before this field.
    #[serde(default)]
    plotted: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Curve {
    x: String,
    y: String,
    panel: u32,
    plot: bool,
}

impl Session {
    /// Paths that are copied into the archive when embedding.
    fn inputs_mut(&mut self) -> [&mut Option<PathBuf>; 2] {
        [&mut self.convert.input, &mut self.inspect.file]
    }
}

/// Directory that holds the extracted inputs of the session at `path`.
pub fn cache_dir(root: &Path, path: &Path) -> PathBuf {
    let mut hasher = DefaultHasher::new();
    path.hash(&mut hasher);
    root.join(format!("{:016x}", hasher.finish()))
}

/// Removes every extracted session; call only while none is in use.
pub fn clear_cache(root: &Path) -> io::Result<()> {
    match fs::remove_dir_all(root) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

/// Writes `session` to `path`; a failure leaves any existing file untouched.
pub fn save(path: &Path, session: &Session) -> Result<()> {
    let dir = parent_dir(path);
    let mut manifest = session.clone();
    let mut files = Vec::new();

    if manifest.embed {
        for (index, slot) in manifest.inputs_mut().into_iter().enumerate() {
            if let Some(source) = slot.take() {
                let name = source
                    .file_name()
                    .ok_or_else(|| format!("{}: not a file", source.display()))?;
                // The index keeps equal file names apart; a directory keeps the
                // entry name stable when a loaded copy is saved again.
                let entry = format!("{INPUTS}/{index}/{}", name.to_string_lossy());
                *slot = Some(PathBuf::from(&entry));
                files.push((entry, source));
            }
        }
    } else {
        manifest
            .inputs_mut()
            .into_iter()
            .for_each(|slot| relativize(slot, dir));
    }
    relativize(&mut manifest.convert.output, dir);

    let mut temp = NamedTempFile::new_in(dir).map_err(|error| describe(dir, error))?;
    write_archive(temp.as_file_mut(), &manifest, &files).map_err(|error| describe(path, error))?;
    temp.persist(path)
        .map_err(|error| describe(path, error.error))?;
    Ok(())
}

/// Reads the session at `path`, extracting embedded inputs into `cache`.
pub fn load(path: &Path, cache: &Path) -> Result<Session> {
    let file = File::open(path).map_err(|error| describe(path, error))?;
    let mut archive = ZipArchive::new(file)
        .map_err(|error| format!("{}: not a ChassisLab session ({error})", path.display()))?;
    if archive.len() > MAX_ENTRIES {
        return Err(format!("{}: too many entries", path.display()));
    }

    let mut session = read_manifest(&mut archive)?;
    let dir = parent_dir(path);
    if session.embed {
        extract(&mut archive, cache)?;
        session
            .inputs_mut()
            .into_iter()
            .for_each(|slot| resolve(slot, cache));
    } else {
        session
            .inputs_mut()
            .into_iter()
            .for_each(|slot| resolve(slot, dir));
    }
    resolve(&mut session.convert.output, dir);
    Ok(session)
}

fn write_archive(
    file: &mut File,
    manifest: &Session,
    files: &[(String, PathBuf)],
) -> std::result::Result<(), Box<dyn Error>> {
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Stored)
        .large_file(true);
    let mut zip = ZipWriter::new(file);
    zip.start_file(MANIFEST, options)?;
    serde_json::to_writer_pretty(&mut zip, manifest)?;
    for (name, source) in files {
        let mut input = File::open(source).map_err(|error| describe(source, error))?;
        zip.start_file(name, options)?;
        io::copy(&mut input, &mut zip)?;
    }
    zip.finish()?;
    Ok(())
}

/// Parses the version before the rest so a newer file reports its version
/// instead of a confusing field error.
fn read_manifest(archive: &mut ZipArchive<File>) -> Result<Session> {
    let entry = archive
        .by_name(MANIFEST)
        .map_err(|_| format!("missing {MANIFEST}"))?;
    let mut text = String::new();
    entry
        .take(MAX_MANIFEST_BYTES)
        .read_to_string(&mut text)
        .map_err(|error| format!("{MANIFEST}: {error}"))?;
    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|error| format!("{MANIFEST}: {error}"))?;

    match value.get("version").and_then(serde_json::Value::as_u64) {
        Some(version) if version == u64::from(VERSION) => {}
        Some(version) => return Err(format!("unsupported session version {version}")),
        None => return Err(format!("{MANIFEST}: missing version")),
    }
    serde_json::from_value(value).map_err(|error| format!("{MANIFEST}: {error}"))
}

/// Replaces `cache` with the archive's `inputs/` entries.
fn extract(archive: &mut ZipArchive<File>, cache: &Path) -> Result<()> {
    match fs::remove_dir_all(cache) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => {
            return Err(describe(cache, error));
        }
        _ => {}
    }

    let mut total = 0;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|error| error.to_string())?;
        let name = entry
            .name()
            .map_err(|error| error.to_string())?
            .into_owned();
        if name == MANIFEST {
            continue;
        }
        let relative = input_path(&name)?;
        if entry.is_dir() {
            continue;
        }
        if entry.is_symlink() {
            return Err(format!("{name}: links are not allowed"));
        }
        total += entry.size();
        if total > MAX_EXTRACTED_BYTES {
            return Err("embedded inputs are too large".into());
        }

        let target = cache.join(relative);
        fs::create_dir_all(target.parent().unwrap_or(cache))
            .map_err(|error| describe(&target, error))?;
        let mut out = File::create(&target).map_err(|error| describe(&target, error))?;
        io::copy(&mut entry, &mut out).map_err(|error| describe(&target, error))?;
    }
    Ok(())
}

/// Accepts only plain relative names below `inputs/`.
fn input_path(name: &str) -> Result<&Path> {
    let path = Path::new(name);
    let mut components = path.components();
    let valid = components.next() == Some(Component::Normal(INPUTS.as_ref()))
        && components.all(|part| matches!(part, Component::Normal(_)));
    if valid {
        Ok(path)
    } else {
        Err(format!("{name}: unexpected entry in session"))
    }
}

fn parent_dir(path: &Path) -> &Path {
    path.parent()
        .filter(|dir| !dir.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
}

/// Stores paths below `dir` relative to it; others stay absolute.
fn relativize(slot: &mut Option<PathBuf>, dir: &Path) {
    if let Some(path) = slot
        && let Ok(relative) = path.strip_prefix(dir)
    {
        *path = relative.to_path_buf();
    }
}

fn resolve(slot: &mut Option<PathBuf>, base: &Path) {
    if let Some(path) = slot
        && path.is_relative()
    {
        *path = base.join(&path);
    }
}

fn describe(path: &Path, error: impl std::fmt::Display) -> String {
    format!("{}: {error}", path.display())
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    fn session(dir: &Path, embed: bool) -> Session {
        Session {
            version: VERSION,
            embed,
            convert: ConvertState {
                input: Some(dir.join("data/run.res")),
                domain: "knc".into(),
                test: Some("vertical".into()),
                output: Some(PathBuf::from("/elsewhere/out")),
                force: true,
            },
            inspect: InspectState {
                file: Some(dir.join("run.h5")),
                curves: vec![Curve {
                    x: "time".into(),
                    y: "lfwcx".into(),
                    panel: 1,
                    plot: true,
                }],
                plotted: true,
            },
        }
    }

    fn write_archive_with(path: &Path, entries: &[(&str, &[u8])]) {
        let mut zip = ZipWriter::new(File::create(path).unwrap());
        for (name, bytes) in entries {
            zip.start_file(*name, SimpleFileOptions::default()).unwrap();
            zip.write_all(bytes).unwrap();
        }
        zip.finish().unwrap();
    }

    fn raw_manifest(path: &Path) -> Session {
        read_manifest(&mut ZipArchive::new(File::open(path).unwrap()).unwrap()).unwrap()
    }

    #[test]
    fn round_trips_linked_session() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.clab");
        let original = session(dir.path(), false);

        save(&path, &original).unwrap();

        assert_eq!(load(&path, &dir.path().join("cache")).unwrap(), original);
    }

    #[test]
    fn stores_paths_below_session_directory_relative() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.clab");

        save(&path, &session(dir.path(), false)).unwrap();

        let stored = raw_manifest(&path);
        assert_eq!(stored.convert.input, Some(PathBuf::from("data/run.res")));
        assert_eq!(stored.inspect.file, Some(PathBuf::from("run.h5")));
        assert_eq!(stored.convert.output, Some(PathBuf::from("/elsewhere/out")));
    }

    #[test]
    fn round_trips_embedded_inputs() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.clab");
        let cache = dir.path().join("cache");
        fs::create_dir(dir.path().join("data")).unwrap();
        fs::write(dir.path().join("data/run.res"), b"result").unwrap();
        fs::write(dir.path().join("run.h5"), b"hdf5").unwrap();

        save(&path, &session(dir.path(), true)).unwrap();
        let loaded = load(&path, &cache).unwrap();

        let input = loaded.convert.input.unwrap();
        let file = loaded.inspect.file.unwrap();
        assert!(input.starts_with(&cache) && file.starts_with(&cache));
        assert_eq!(fs::read(input).unwrap(), b"result");
        assert_eq!(fs::read(file).unwrap(), b"hdf5");
    }

    #[test]
    fn keeps_entry_names_stable_across_resaves() {
        let dir = tempfile::tempdir().unwrap();
        let first = dir.path().join("a.clab");
        let second = dir.path().join("b.clab");
        let cache = dir.path().join("cache");
        fs::create_dir(dir.path().join("data")).unwrap();
        fs::write(dir.path().join("data/run.res"), b"result").unwrap();
        fs::write(dir.path().join("run.h5"), b"hdf5").unwrap();

        save(&first, &session(dir.path(), true)).unwrap();
        save(&second, &load(&first, &cache).unwrap()).unwrap();

        assert_eq!(raw_manifest(&second), raw_manifest(&first));
        assert_eq!(
            raw_manifest(&second).inspect.file,
            Some(PathBuf::from("inputs/1/run.h5"))
        );
    }

    #[test]
    fn loads_sessions_saved_without_plotted() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.clab");
        let mut manifest = serde_json::to_value(session(dir.path(), false)).unwrap();
        manifest["inspect"]
            .as_object_mut()
            .unwrap()
            .remove("plotted");
        write_archive_with(
            &path,
            &[(MANIFEST, &serde_json::to_vec(&manifest).unwrap())],
        );

        let loaded = load(&path, &dir.path().join("cache")).unwrap();

        assert!(!loaded.inspect.plotted);
    }

    #[test]
    fn clears_cache_and_tolerates_missing_one() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("sessions");
        fs::create_dir_all(root.join("abc/inputs")).unwrap();

        clear_cache(&root).unwrap();
        assert!(!root.exists());
        clear_cache(&root).unwrap();
    }

    #[test]
    fn rejects_unsupported_version() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.clab");
        write_archive_with(&path, &[(MANIFEST, br#"{"version": 2}"#)]);

        let error = load(&path, &dir.path().join("cache")).unwrap_err();

        assert!(error.contains("version 2"), "{error}");
    }

    #[test]
    fn rejects_entries_outside_inputs() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.clab");
        let cache = dir.path().join("cache");
        let manifest = serde_json::to_vec(&session(dir.path(), true)).unwrap();
        write_archive_with(&path, &[(MANIFEST, &manifest), ("inputs/../evil", b"x")]);

        let error = load(&path, &cache).unwrap_err();

        assert!(error.contains("unexpected entry"), "{error}");
        assert!(!dir.path().join("evil").exists());
    }

    #[test]
    fn failed_save_keeps_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.clab");
        fs::write(&path, b"old").unwrap();

        // The embedded input does not exist, so writing fails midway.
        assert!(save(&path, &session(dir.path(), true)).is_err());

        assert_eq!(fs::read(&path).unwrap(), b"old");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }
}
