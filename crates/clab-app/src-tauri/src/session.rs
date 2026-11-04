//! `.clab` session files: a zip archive holding `session.json` and, when the
//! session embeds its files and reports, copies of them under `inputs/`.
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
const MAX_ENTRIES: usize = 10_000;
const MAX_MANIFEST_BYTES: u64 = 1 << 20;
const MAX_EXTRACTED_BYTES: u64 = 64 << 30;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Session {
    version: u32,
    /// Whether input files and existing report folders are copied on save.
    embed: bool,
    convert: ConvertState,
    inspect: InspectState,
    analyze: AnalyzeState,
}

/// The Analyze form. Result cards are never stored, and `--force` is asked
/// again on every run. Existing report folders can be embedded separately.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AnalyzeState {
    config: Option<PathBuf>,
    /// One input is analyzed; more are compared, the first being the baseline.
    inputs: Vec<PathBuf>,
    tolerances: bool,
    generate: bool,
    report: Option<PathBuf>,
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
    /// Whether the chart was drawn.
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
    pub fn embedded_paths(&self, cache: &Path) -> Vec<PathBuf> {
        [
            &self.convert.input,
            &self.inspect.file,
            &self.analyze.report,
        ]
        .into_iter()
        .flatten()
        .filter(|path| self.embed && path.starts_with(cache))
        .cloned()
        .collect()
    }

    /// Inputs or an Inspect report folder, copied when embedding.
    fn inputs_mut(&mut self) -> [&mut Option<PathBuf>; 2] {
        [&mut self.convert.input, &mut self.inspect.file]
    }

    /// Linked paths, including the report destination when it is not embedded.
    fn linked_mut(&mut self) -> impl Iterator<Item = &mut PathBuf> {
        [
            &mut self.convert.output,
            &mut self.analyze.config,
            &mut self.analyze.report,
        ]
        .into_iter()
        .flatten()
        .chain(&mut self.analyze.inputs)
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
                collect_files(&source, &entry, &mut files)?;
            }
        }
    } else {
        manifest
            .inputs_mut()
            .into_iter()
            .flatten()
            .for_each(|path| relativize(path, dir));
    }
    manifest.linked_mut().for_each(|path| relativize(path, dir));

    // An Analyze report is also an output destination: a folder that has not
    // been generated yet remains linked rather than making Save fail.
    if session.embed
        && let Some(source) = &session.analyze.report
    {
        match fs::symlink_metadata(source) {
            Ok(metadata) => {
                if !metadata.is_dir() || metadata.is_symlink() {
                    return Err(describe(
                        source,
                        "expected a report folder, not a file or link",
                    ));
                }
                let name = source
                    .file_name()
                    .ok_or_else(|| describe(source, "not a report folder"))?;
                let entry = format!("{INPUTS}/2/{}", name.to_string_lossy());
                collect_files(source, &entry, &mut files)?;
                manifest.analyze.report = Some(entry.into());
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(describe(source, error)),
        }
    }

    let mut temp = NamedTempFile::new_in(dir).map_err(|error| describe(dir, error))?;
    write_archive(temp.as_file_mut(), &manifest, &files)
        .map_err(|error| format!("Could not save session {}: {error}", path.display()))?;
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
        for path in session.inputs_mut().into_iter().flatten() {
            input_path(&path.to_string_lossy())?;
        }
        extract(&mut archive, cache)?;
        session
            .inputs_mut()
            .into_iter()
            .flatten()
            .for_each(|path| resolve(path, cache));
    } else {
        session
            .inputs_mut()
            .into_iter()
            .flatten()
            .for_each(|path| resolve(path, dir));
    }
    // An embedded Analyze report is an archive entry; a linked one resolves below.
    if session.embed
        && let Some(report) = &mut session.analyze.report
        && report.starts_with(INPUTS)
    {
        input_path(&report.to_string_lossy())?;
        *report = cache.join(&report);
    }
    session.linked_mut().for_each(|path| resolve(path, dir));
    Ok(session)
}

/// Collect plain files only; never follow links out of a report tree.
/// Directory entries preserve empty folders and stable names across resaves.
fn collect_files(source: &Path, entry: &str, files: &mut Vec<(String, PathBuf)>) -> Result<()> {
    input_path(entry).map_err(|error| describe(source, error))?;
    if entry.split('/').count() > 128 {
        return Err(describe(source, "embedded folder nesting is too deep"));
    }
    let metadata = fs::symlink_metadata(source).map_err(|error| describe(source, error))?;
    if metadata.is_symlink() || !(metadata.is_file() || metadata.is_dir()) {
        return Err(describe(
            source,
            "only regular files and folders can be embedded; links are not allowed",
        ));
    }
    if files.len() + 1 >= MAX_ENTRIES {
        return Err(describe(source, "too many embedded entries"));
    }
    files.push((
        if metadata.is_dir() {
            format!("{entry}/")
        } else {
            entry.into()
        },
        source.into(),
    ));
    if metadata.is_dir() {
        let children = fs::read_dir(source).map_err(|error| describe(source, error))?;
        for child in children {
            let child = child.map_err(|error| describe(source, error))?;
            let name = child.file_name();
            let name = name
                .to_str()
                .ok_or_else(|| describe(&child.path(), "filename is not UTF-8"))?;
            collect_files(&child.path(), &format!("{entry}/{name}"), files)?;
        }
    }
    Ok(())
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
    let mut total = 0;
    for (name, source) in files {
        if name.ends_with('/') {
            zip.add_directory(name, options)?;
            continue;
        }
        let mut input = File::open(source).map_err(|error| describe(source, error))?;
        zip.start_file(name, options)?;
        let copied = io::copy(
            &mut (&mut input).take(MAX_EXTRACTED_BYTES - total + 1),
            &mut zip,
        )
        .map_err(|error| describe(source, error))?;
        total += copied;
        if total > MAX_EXTRACTED_BYTES {
            return Err(describe(source, "embedded files and reports are too large").into());
        }
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
        let entry = archive.by_index(index).map_err(|error| error.to_string())?;
        let name = entry
            .name()
            .map_err(|error| error.to_string())?
            .into_owned();
        if name == MANIFEST {
            continue;
        }
        let relative = input_path(&name)?;
        if entry.is_symlink() {
            return Err(format!("{name}: links are not allowed"));
        }
        if entry.is_dir() {
            fs::create_dir_all(cache.join(relative))
                .map_err(|error| describe(&cache.join(relative), error))?;
            continue;
        }
        if entry.size() > MAX_EXTRACTED_BYTES - total {
            return Err("embedded files and reports are too large".into());
        }

        let target = cache.join(relative);
        fs::create_dir_all(target.parent().unwrap_or(cache))
            .map_err(|error| describe(&target, error))?;
        let mut out = File::options()
            .write(true)
            .create_new(true)
            .open(&target)
            .map_err(|error| describe(&target, error))?;
        total += io::copy(&mut entry.take(MAX_EXTRACTED_BYTES - total + 1), &mut out)
            .map_err(|error| describe(&target, error))?;
        if total > MAX_EXTRACTED_BYTES {
            return Err("embedded files and reports are too large".into());
        }
    }
    Ok(())
}

/// Accepts only plain relative names below `inputs/`.
fn input_path(name: &str) -> Result<&Path> {
    let path = Path::new(name);
    let mut components = path.components();
    let valid = !name.contains(['\\', ':'])
        && !name.split('/').any(|part| matches!(part, "." | ".."))
        && components.next() == Some(Component::Normal(INPUTS.as_ref()))
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
fn relativize(path: &mut PathBuf, dir: &Path) {
    if let Ok(relative) = path.strip_prefix(dir) {
        *path = relative.to_path_buf();
    }
}

fn resolve(path: &mut PathBuf, base: &Path) {
    if path.is_relative() {
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
            analyze: AnalyzeState {
                config: Some(dir.join("config.toml")),
                inputs: vec![dir.join("data/run.toml"), PathBuf::from("/elsewhere/b.h5")],
                tolerances: true,
                generate: true,
                report: Some(PathBuf::from("/elsewhere/report")),
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
        assert_eq!(stored.analyze.config, Some(PathBuf::from("config.toml")));
        assert_eq!(
            stored.analyze.inputs,
            [
                PathBuf::from("data/run.toml"),
                PathBuf::from("/elsewhere/b.h5")
            ]
        );
        assert_eq!(
            stored.analyze.report,
            Some(PathBuf::from("/elsewhere/report"))
        );
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

    /// Inspect shows `report`, Analyze wrote `analysis`; no other input is set.
    fn report_session(dir: &Path) -> Session {
        for report in ["report", "analysis"] {
            let root = dir.join(report);
            fs::create_dir_all(root.join("plots/knc")).unwrap();
            fs::create_dir(root.join("empty")).unwrap();
            fs::write(root.join("report.json"), report).unwrap();
            fs::write(root.join("plots/knc/a.pdf"), b"pdf").unwrap();
        }
        let mut session = session(dir, true);
        session.convert.input = None;
        session.inspect.file = Some(dir.join("report"));
        session.analyze.report = Some(dir.join("analysis"));
        session
    }

    #[test]
    fn round_trips_embedded_report_folders() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.clab");
        let cache = dir.path().join("cache");

        save(&path, &report_session(dir.path())).unwrap();
        let loaded = load(&path, &cache).unwrap();

        let inspect = loaded.inspect.file.clone().unwrap();
        let analyze = loaded.analyze.report.clone().unwrap();
        assert_eq!(inspect, cache.join("inputs/1/report"));
        assert_eq!(analyze, cache.join("inputs/2/analysis"));
        assert_eq!(fs::read(inspect.join("report.json")).unwrap(), b"report");
        assert_eq!(fs::read(analyze.join("report.json")).unwrap(), b"analysis");
        assert_eq!(fs::read(inspect.join("plots/knc/a.pdf")).unwrap(), b"pdf");
        assert!(inspect.join("empty").is_dir());
        assert_eq!(loaded.embedded_paths(&cache), vec![inspect, analyze]);
    }

    #[test]
    fn keeps_report_entry_names_stable_across_resaves() {
        let dir = tempfile::tempdir().unwrap();
        let first = dir.path().join("a.clab");
        let second = dir.path().join("b.clab");

        save(&first, &report_session(dir.path())).unwrap();
        save(&second, &load(&first, &dir.path().join("cache")).unwrap()).unwrap();

        assert_eq!(raw_manifest(&second), raw_manifest(&first));
        assert_eq!(
            raw_manifest(&second).analyze.report,
            Some(PathBuf::from("inputs/2/analysis"))
        );
    }

    #[test]
    fn keeps_unwritten_analyze_report_linked() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.clab");
        let mut session = report_session(dir.path());
        session.analyze.report = Some(dir.path().join("not-yet"));

        save(&path, &session).unwrap();

        assert_eq!(
            raw_manifest(&path).analyze.report,
            Some(PathBuf::from("not-yet"))
        );
        let loaded = load(&path, &dir.path().join("cache")).unwrap();
        assert_eq!(loaded.analyze.report, Some(dir.path().join("not-yet")));
    }

    #[cfg(unix)]
    #[test]
    fn rejects_links_in_report_and_names_the_path() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.clab");
        let session = report_session(dir.path());
        let link = dir.path().join("report/secret");
        std::os::unix::fs::symlink("/etc/passwd", &link).unwrap();

        let error = save(&path, &session).unwrap_err();

        assert!(error.contains(&link.display().to_string()), "{error}");
        assert!(!path.exists());
    }

    #[test]
    fn rejects_embedded_report_path_escaping_the_cache() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.clab");
        let mut manifest = report_session(dir.path());
        manifest.analyze.report = Some(PathBuf::from("inputs/../../escape"));
        write_archive_with(
            &path,
            &[(MANIFEST, &serde_json::to_vec(&manifest).unwrap())],
        );

        assert!(load(&path, &dir.path().join("cache")).is_err());
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
