//! Typed, validated case configuration.
//!
//! Split by schema generation, all reachable from [`load_config`]:
//! - [`function_first`] parses today's canonical `[input]`-table schema.
//! - [`legacy`] parses the older unified (`[svc]`/`[vertical]`) and flat
//!   schemas, and owns `VerticalConfig` construction — `function_first`
//!   builds an equivalent `[vertical]`-shaped table for each KnC test and
//!   delegates to `legacy::parse_vertical` rather than duplicating it.
//! - [`support`] holds the generic TOML/JSON helpers and signal-mapping
//!   resolution shared by both.

mod function_first;
mod legacy;
mod support;

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use thiserror::Error;
use toml::Value;

pub use legacy::{VerticalBranch, VerticalBranches, VerticalConfig, VerticalSetup, VerticalWindow};
use support::{absolute, expand, merge_map, table};

#[derive(Debug, Clone, PartialEq, Error)]
#[error("{0}")]
pub struct ConfigError(pub String);

#[derive(Debug, Clone, PartialEq)]
pub struct SignalRef {
    pub entity: String,
    pub component: String,
    pub unit: String,
}
/// Raw-signal mapping for a measurement source: a flat channel or JSON key
/// name, unlike simulation's entity+component pair. Shared by every
/// measurement-mapping table (KnC vertical HDF5 channels, SVC reference
/// JSON keys).
#[derive(Debug, Clone, PartialEq)]
pub struct MeasurementSignalRef {
    pub entity: String,
    pub unit: String,
}
#[derive(Debug, Clone, PartialEq)]
pub struct ValidationTolerances {
    pub absolute: BTreeMap<String, f64>,
    pub relative_percent: BTreeMap<String, f64>,
}
#[derive(Debug, Clone, PartialEq)]
pub struct ValidationConfig {
    pub reference: PathBuf,
    pub tolerances: ValidationTolerances,
}

/// KnC tests analyzed from one configured input each. `static` is not listed:
/// it is only a baseline for these tests.
pub const KNC_TESTS: [&str; 2] = ["vertical", "roll"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Domain {
    Svc,
    Knc,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceKind {
    Simulation,
    Measurement,
}
#[derive(Debug, Clone, PartialEq)]
pub struct CaseConfig {
    pub path: PathBuf,
    pub name: String,
    pub loadcase: String,
    pub simulation_json: PathBuf,
    pub svc_source_kind: Option<SourceKind>,
    pub gravity: f64,
    pub signals: BTreeMap<String, SignalRef>,
    pub validation: Option<ValidationConfig>,
    pub vertical: Option<VerticalConfig>,
    /// KnC roll test; shares the vertical test's configuration shape.
    pub roll: Option<VerticalConfig>,
    pub svc_signal_map: Option<PathBuf>,
    /// SVC measurement reference JSON key mapping: canonical metric name ->
    /// `{entity (JSON key), unit}`. Required whenever this config's SVC
    /// input is a measurement reference — there is no hardcoded canonical
    /// name fallback.
    pub svc_measurement_signals: BTreeMap<String, MeasurementSignalRef>,
}
impl CaseConfig {
    pub fn configured_domains(&self) -> Vec<Domain> {
        let mut domains = Vec::new();
        if !self.simulation_json.as_os_str().is_empty() {
            domains.push(Domain::Svc);
        }
        if self.vertical.is_some() || self.roll.is_some() {
            domains.push(Domain::Knc);
        }
        domains
    }
}
/// Signal-map file path (if external) plus the parsed signal mapping,
/// shared by the legacy and unified schema parsers.
pub(crate) type SignalSource = (Option<PathBuf>, BTreeMap<String, SignalRef>);

/// Load and validate one case configuration.
///
/// The root schema selects parsing mode: function-first configurations have an
/// `[input]` table, unified configurations have `[svc]` or `[vertical]`, and
/// older flat configurations use the legacy schema. All referenced paths are
/// resolved relative to the configuration file.
pub fn load_config(path: impl AsRef<Path>) -> Result<CaseConfig, Box<dyn std::error::Error>> {
    let (path, raw) = load_toml(path.as_ref())?;
    parse_root(&raw, &path)
}

fn parse_root(
    root: &toml::map::Map<String, Value>,
    path: &Path,
) -> Result<CaseConfig, Box<dyn std::error::Error>> {
    if root.contains_key("input") {
        function_first::parse_function_first(root, path)
    } else if root.contains_key("svc") || root.contains_key("vertical") {
        legacy::parse_unified(root, path)
    } else {
        legacy::parse_legacy(root, path)
    }
}

/// Load shared analysis configuration and one run manifest.
///
/// External role files are expanded before both documents are merged, so each
/// role keeps paths relative to its own TOML file. Run metadata identifies each
/// run; absent metadata is reported as `undefined`.
pub fn load_config_and_run(
    config: impl AsRef<Path>,
    run: impl AsRef<Path>,
) -> Result<CaseConfig, Box<dyn std::error::Error>> {
    let (config_path, config_root) = load_toml(config.as_ref())?;
    let (run_path, run_root) = load_toml(run.as_ref())?;
    let mut config_root = function_first::load_function_role_files(&config_root, &config_path)?;
    // Run metadata never inherits from reusable shared configuration.
    config_root.remove("meta");
    config_root.remove("case");
    let mut run_root = Value::Table(run_root);
    function_first::resolve_external_paths(
        &mut run_root,
        run_path.parent().unwrap_or_else(|| Path::new(".")),
    )?;
    let run_root =
        function_first::load_function_role_files(table(&run_root, "run manifest")?, &run_path)?;
    let mut root = config_root;
    merge_map(&mut root, &run_root);
    parse_root(&root, &config_path)
}

pub fn validate_run_input_files(run: impl AsRef<Path>) -> Result<(), Box<dyn std::error::Error>> {
    let (run, root) = load_toml(run.as_ref())?;
    validate_run_input_tables(&root, &run)
}

fn validate_run_input_tables(
    root: &toml::Table,
    run: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let Some(input) = root.get("input").and_then(Value::as_table) else {
        return Ok(());
    };
    if let Some(svc) = input.get("svc").and_then(Value::as_table) {
        validate_run_input_table(svc, run)?;
    }
    if let Some(knc) = input.get("knc").and_then(Value::as_table) {
        for test in KNC_TESTS {
            if let Some(input) = knc.get(test).and_then(Value::as_table) {
                validate_run_input_table(input, run)?;
            }
        }
        if let Some(static_input) = knc.get("static").and_then(Value::as_table) {
            validate_run_input_table(static_input, run)?;
        }
    }
    Ok(())
}

fn validate_run_input_table(
    input: &toml::map::Map<String, Value>,
    run: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    for key in ["file", "input_file", "static_file"] {
        let Some(file) = input.get(key).and_then(Value::as_str) else {
            continue;
        };
        let path = expand(Path::new(file))?;
        let path = if path.is_absolute() {
            path
        } else {
            run.parent().unwrap_or_else(|| Path::new(".")).join(path)
        };
        if !path.is_file() {
            return Err(ConfigError(format!("{file} cannot be found")).into());
        }
    }
    for key in ["simulation", "measurement", "static_simulation"] {
        if let Some(input) = input.get(key).and_then(Value::as_table) {
            validate_run_input_table(input, run)?;
        }
    }
    Ok(())
}

/// Load shared analysis configuration for a direct normalized input.
///
/// `knc_test` names the KnC test the input belongs to, `None` for SVC. A KnC
/// input supplies the same file in both roles solely to build its typed
/// mapping/setup configuration. The caller must only read its actual source.
/// Direct input has no run manifest, so absent `[meta]` name and loadcase are
/// reported as `undefined`.
pub fn load_config_with_input(
    config: impl AsRef<Path>,
    input: impl AsRef<Path>,
    knc_test: Option<&str>,
) -> Result<CaseConfig, Box<dyn std::error::Error>> {
    let (path, raw) = load_toml(config.as_ref())?;
    let mut root = function_first::load_function_role_files(&raw, &path)?;
    let input = absolute(&expand(input.as_ref())?)?.display().to_string();
    let mut role = toml::map::Map::new();
    if let Some(test) = knc_test {
        let mut test_role = toml::map::Map::new();
        test_role.insert("file".into(), Value::String(input.clone()));
        let mut knc = toml::map::Map::new();
        knc.insert(test.into(), Value::Table(test_role));
        role.insert("knc".into(), Value::Table(knc));
    } else {
        let mut svc = toml::map::Map::new();
        svc.insert("file".into(), Value::String(input));
        role.insert("svc".into(), Value::Table(svc));
    }
    if let Some(test) = knc_test
        && let Some(existing) = root
            .get_mut("input")
            .and_then(Value::as_table_mut)
            .and_then(|input| input.get_mut("knc"))
            .and_then(Value::as_table_mut)
            .and_then(|knc| knc.get_mut(test))
            .and_then(Value::as_table_mut)
    {
        // Direct input replaces active source, but keeps configured static input.
        existing.remove("file");
        existing.remove("simulation");
        existing.remove("measurement");
    }
    match root.get_mut("input") {
        Some(Value::Table(existing)) => merge_map(existing, &role),
        Some(_) => return Err(ConfigError("[input] must be a table".into()).into()),
        None => {
            root.insert("input".into(), Value::Table(role));
        }
    }
    function_first::parse_function_first(&root, &path)
}

fn load_toml(path: &Path) -> Result<(PathBuf, toml::Table), Box<dyn std::error::Error>> {
    let path = absolute(&expand(path)?)?;
    if !path.is_file() {
        return Err(format!("config file does not exist: {}", path.display()).into());
    }
    let text = fs::read_to_string(&path)?;
    let raw = text
        .parse()
        .map_err(|e| ConfigError(format!("invalid TOML in {}: {e}", path.display())))?;
    Ok((path, raw))
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use hdf5::File;
    use hdf5::types::VarLenUnicode;

    use super::*;

    fn source_kind(path: &Path, value: &str) {
        let file = File::create(path).unwrap();
        match value {
            "simulation" => {
                scalar(&file, "format_version", 3_u32);
                scalar(&file, "sample_count", 0_u64);
                scalar(&file, "signal_count", 0_u64);
                string(&file, "source_kind", "simulation");
                string(&file, "source_file", "result.res");
                file.create_group("results").unwrap();
            }
            "measurement" => {
                scalar(&file, "format_version", 1_u32);
                scalar(&file, "sample_count", 0_u64);
                scalar(&file, "channel_count", 0_u64);
                string(&file, "source_kind", "measurement");
                string(&file, "source_file", "measurement.acd");
                let measurements = file.create_group("measurements").unwrap();
                measurements
                    .new_dataset_builder()
                    .with_data(&[] as &[f64])
                    .create("sample_axis")
                    .unwrap();
                measurements.create_group("channels").unwrap();
            }
            _ => panic!("unsupported source kind {value}"),
        }
        file.flush().unwrap();
    }

    fn scalar<T: hdf5::H5Type>(file: &File, name: &str, value: T) {
        file.new_attr::<T>()
            .create(name)
            .unwrap()
            .write_scalar(&value)
            .unwrap();
    }

    fn string(file: &File, name: &str, value: &str) {
        file.new_attr::<VarLenUnicode>()
            .shape(())
            .create(name)
            .unwrap()
            .write_scalar(&VarLenUnicode::from_str(value).unwrap())
            .unwrap();
    }
    #[test]
    fn parses_legacy_inline() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("case.toml");
        fs::write(&p,"[case]\nname='n'\ndomain='svc'\nloadcase='l'\n[inputs]\nsimulation_json='x.json'\n[physics]\ngravity=9.8\n[signals.x]\nentity='e'\ncomponent='c'\nunit='u'\n").unwrap();
        let c = load_config(&p).unwrap();
        assert_eq!(c.signals["x"].entity, "e");
        assert_eq!(c.simulation_json, d.path().join("x.json"));
    }

    #[test]
    fn parses_legacy_runtime_smoke_fixture() {
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures");
        let directory = tempfile::tempdir().unwrap();
        let fixture = directory.path().join("runtime-smoke.toml");
        fs::copy(fixtures.join("runtime-smoke.toml"), &fixture).unwrap();
        fs::copy(
            fixtures.join("runtime-smoke-signals.toml"),
            directory.path().join("example-signals.toml"),
        )
        .unwrap();

        let config = load_config(fixture).unwrap();

        assert_eq!(config.name, "example-config");
        assert!(config.svc_signal_map.is_some());
        assert!(config.vertical.is_some());
    }

    #[test]
    fn rejects_unknown_and_bad_tolerance() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("case.toml");
        fs::write(&p,"[case]\nname='n'\ndomain='svc'\nloadcase='l'\nextra=1\n[inputs]\nsimulation_json='x.json'\n[physics]\ngravity=9.8\n").unwrap();
        assert!(load_config(&p).unwrap_err().to_string().contains("unknown"));
    }

    #[test]
    fn parses_modular_signal_mappings_and_tolerances() {
        let d = tempfile::tempdir().unwrap();
        fs::write(
            d.path().join("signals.toml"),
            "[signals.total]\nentity='e'\ncomponent='c'\nunit='kg'\n",
        )
        .unwrap();
        fs::write(
            d.path().join("tolerances.toml"),
            "[absolute]\ntotal_mass_kg=1\n",
        )
        .unwrap();
        let p = d.path().join("case.toml");
        fs::write(
            &p,
            "[case]\nname='n'\nloadcase='l'\n[physics]\ngravity=9.8\n[svc.inputs]\nsimulation_json='simulation.json'\nmeasurement_reference='reference.json'\n[svc.mappings]\nfile='signals.toml'\n[svc.validation]\ntolerances='tolerances.toml'\n",
        )
        .unwrap();

        let config = load_config(&p).unwrap();
        assert_eq!(config.signals["total"].unit, "kg");
        assert_eq!(
            config.validation.unwrap().tolerances.absolute["total_mass_kg"],
            1.0
        );
    }

    #[test]
    fn rejects_modular_and_inline_signal_mappings_together() {
        let d = tempfile::tempdir().unwrap();
        fs::write(
            d.path().join("signals.toml"),
            "[signals.total]\nentity='e'\ncomponent='c'\nunit='kg'\n",
        )
        .unwrap();
        let p = d.path().join("case.toml");
        fs::write(
            &p,
            "[case]\nname='n'\nloadcase='l'\n[physics]\ngravity=9.8\n[svc.inputs]\nsimulation_json='simulation.json'\n[svc.mappings]\nfile='signals.toml'\n[svc.mappings.total]\nentity='e'\ncomponent='c'\nunit='kg'\n",
        )
        .unwrap();

        assert!(load_config(&p).unwrap_err().to_string().contains("both"));
    }
    #[test]
    fn parses_vertical_only_unified_config() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("case.toml");
        fs::write(&p, "[case]\nname='n'\nloadcase='l'\n[vertical.inputs]\nmeasurement_hdf5='measurement.h5'\nsimulation_hdf5='simulation.h5'\n[vertical.setup]\nbrakes_applied=true\nengine_running=false\nfront_arb_connected=true\n[vertical.window]\nminimum_mm=-1\nmaximum_mm=1\nstep_mm=1\n[vertical.signals.x]\nentity='e'\ncomponent='c'\nunit='mm'\n[vertical.measurement_signals.x]\nentity='e'\nunit='mm'\n").unwrap();
        let config = load_config(&p).unwrap();
        assert_eq!(config.configured_domains(), vec![Domain::Knc]);
    }

    #[test]
    fn validates_vertical_branch_options() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("case.toml");
        let base = "[case]\nname='n'\nloadcase='l'\n[vertical.inputs]\nmeasurement_hdf5='measurement.h5'\nsimulation_hdf5='simulation.h5'\n[vertical.setup]\nbrakes_applied=true\nengine_running=false\nfront_arb_connected=true\n[vertical.window]\nminimum_mm=-10\nmaximum_mm=10\nstep_mm=1\n[vertical.signals.x]\nentity='e'\ncomponent='c'\nunit='mm'\n[vertical.measurement_signals.x]\nentity='e'\nunit='mm'\n";
        fs::write(&p, base).unwrap();
        let branch = load_config(&p).unwrap().vertical.unwrap().branch;
        assert_eq!((branch.measurement.index, branch.simulation.index), (2, 2));
        fs::write(
            &p,
            format!("{base}[vertical.branch]\nindex=1\n[vertical.branch.simulation]\nplot_branches='selected'\n"),
        )
        .unwrap();
        let branch = load_config(&p).unwrap().vertical.unwrap().branch;
        assert_eq!((branch.measurement.index, branch.simulation.index), (1, 1));
        assert_eq!(branch.measurement.plot_branches, "all");
        assert_eq!(branch.simulation.plot_branches, "selected");
        for invalid in [
            "index=0",
            "smooth_window=5",
            "min_samples=5",
            "plot_branches='other'",
        ] {
            fs::write(&p, format!("{base}[vertical.branch]\n{invalid}\n")).unwrap();
            assert!(load_config(&p).is_err(), "accepted {invalid}");
        }
    }

    #[test]
    fn parses_vertical_travel_window_and_metric_gradient_points() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("case.toml");
        let write = |window: &str| {
            fs::write(&p, format!("[case]\nname='n'\nloadcase='l'\n[vertical.inputs]\nmeasurement_hdf5='measurement.h5'\nsimulation_hdf5='simulation.h5'\n[vertical.setup]\nbrakes_applied=true\nengine_running=false\nfront_arb_connected=true\n[vertical.window]\n{window}\n[vertical.signals.x]\nentity='e'\ncomponent='c'\nunit='mm'\n[vertical.measurement_signals.x]\nentity='e'\nunit='mm'\n")).unwrap();
        };
        write(
            "travel_minimum_mm=-10\ntravel_maximum_mm=10\nstep_mm=1\nmetric_gradient_points_mm=5",
        );
        let window = load_config(&p).unwrap().vertical.unwrap().window;
        assert_eq!(
            (window.minimum_mm, window.maximum_mm, window.step_mm),
            (-10.0, 10.0, 1.0)
        );
        assert_eq!(window.gradient_points_mm, Some(5.0));
        write("minimum_mm=-1\nmaximum_mm=1\nstep_mm=1");
        let window = load_config(&p).unwrap().vertical.unwrap().window;
        assert_eq!(window.gradient_points_mm, None);
        for (window, error) in [
            (
                "travel_minimum_mm=-1\nminimum_mm=-1\ntravel_maximum_mm=1\nstep_mm=1",
                "cannot define both travel_minimum_mm and minimum_mm",
            ),
            (
                "travel_minimum_mm=-10\ntravel_maximum_mm=10\nstep_mm=1\nmetric_gradient_points_mm=0",
                "gradient_points_mm must be positive",
            ),
            (
                "travel_minimum_mm=-10\ntravel_maximum_mm=4\nstep_mm=1\nmetric_gradient_points_mm=5",
                "must lie inside travel window",
            ),
        ] {
            write(window);
            let message = load_config(&p).unwrap_err().to_string();
            assert!(message.contains(error), "{message}");
        }
    }

    #[test]
    fn parses_sprung_mass_fallback_per_corner() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("case.toml");
        fs::write(
            &p,
            "[case]\nname='n'\nloadcase='l'\n[vertical.inputs]\nmeasurement_hdf5='measurement.h5'\nsimulation_hdf5='simulation.h5'\n[vertical.setup]\nbrakes_applied=true\nengine_running=false\nfront_arb_connected=true\n[vertical.window]\nminimum_mm=-10\nmaximum_mm=10\nstep_mm=1\n[vertical.metric]\nsprung_mass_front_left_kg=449.0\nsprung_mass_rear_right_kg=383.0\n[vertical.signals.x]\nentity='e'\ncomponent='c'\nunit='mm'\n[vertical.measurement_signals.x]\nentity='e'\nunit='mm'\n",
        )
        .unwrap();
        let window = load_config(&p).unwrap().vertical.unwrap().window;
        assert_eq!(window.sprung_mass_kg.get("front_left"), Some(&449.0));
        assert_eq!(window.sprung_mass_kg.get("rear_right"), Some(&383.0));
        assert_eq!(window.sprung_mass_kg.get("front_right"), None);
        assert_eq!(window.sprung_mass_kg.len(), 2);
    }

    #[test]
    fn parses_external_function_first_role_files() {
        let d = tempfile::tempdir().unwrap();
        fs::write(
            d.path().join("input.json"),
            r#"{"svc":{"simulation":{"input_file":"simulation.json"}}}"#,
        )
        .unwrap();
        fs::write(
            d.path().join("mappings.toml"),
            "[simulation.svc.total]\nentity='e'\ncomponent='c'\nunit='kg'\n[measurement.svc.total]\nentity='e'\nunit='kg'\n",
        )
        .unwrap();
        fs::write(d.path().join("validation.toml"), "[svc]\n").unwrap();
        let p = d.path().join("case.toml");
        fs::write(
            &p,
            "[meta]\nname='n'\nloadcase='l'\n[input]\ninput_file='input.json'\n[mappings]\ninput_file='mappings.toml'\n[validation]\ninput_file='validation.toml'\n",
        )
        .unwrap();

        let config = load_config(&p).unwrap();
        assert_eq!(config.simulation_json, d.path().join("simulation.json"));
        assert!(config.validation.is_none());
        assert_eq!(config.signals["total"].component, "c");
    }

    #[test]
    fn splits_simulation_and_measurement_mappings_into_separate_files() {
        let d = tempfile::tempdir().unwrap();
        fs::write(
            d.path().join("svc-simulation-mappings.toml"),
            "[svc.front_left_wheel_load]\nentity='e'\ncomponent='c'\nunit='kg'\n",
        )
        .unwrap();
        fs::write(
            d.path().join("svc-measurement-mappings.toml"),
            "[front_left_mass_kg]\nentity='e2'\nunit='kg'\n",
        )
        .unwrap();
        let p = d.path().join("case.toml");
        fs::write(
            &p,
            "[meta]\nname='n'\nloadcase='l'\n[physics]\ngravity=9.8\n[input.svc.simulation]\nfile='simulation.json'\n\
             [mappings.simulation.svc]\nfile='svc-simulation-mappings.toml'\n\
             [mappings.measurement.svc]\nfile='svc-measurement-mappings.toml'\n",
        )
        .unwrap();

        let config = load_config(&p).unwrap();
        assert_eq!(config.signals["front_left_wheel_load"].entity, "e");
        assert_eq!(
            config.svc_measurement_signals["front_left_mass_kg"].entity,
            "e2"
        );
    }

    #[test]
    fn splits_knc_domain_shared_and_vertical_test_mappings_into_separate_files() {
        let d = tempfile::tempdir().unwrap();
        // Domain-shared (every knc test, not just vertical).
        fs::write(
            d.path().join("knc-simulation-mappings.toml"),
            "[knc.front_left_travel]\nentity='fl'\ncomponent='travel'\nunit='mm'\n",
        )
        .unwrap();
        // Vertical-test-specific.
        fs::write(
            d.path().join("knc-vertical-simulation-mappings.toml"),
            "[front_left_wheel_center_x]\nentity='fl'\ncomponent='x'\nunit='mm'\n",
        )
        .unwrap();
        fs::write(
            d.path().join("knc-vertical-measurement-mappings.toml"),
            "[front_left_wheel_center_x]\nentity='lfwcx'\nunit='mm'\n",
        )
        .unwrap();
        // Function-first `[input]`/`[mappings]` path, not the raw `[vertical]`
        // literal path (whose own inline `signals`/`measurement_signals`
        // keys bypass `merged_mapping` entirely and are unrelated here).
        let input = d.path().join("input.toml");
        fs::write(&input, "[knc.vertical.simulation]\nfile='simulation.h5'\n").unwrap();
        let setup = d.path().join("setup.toml");
        fs::write(
            &setup,
            "[knc.vertical]\nbrakes_applied=true\nengine_running=false\nfront_arb_connected=true\n\
             [knc.vertical.window]\nminimum_mm=-1\nmaximum_mm=1\nstep_mm=1\n",
        )
        .unwrap();
        let case = d.path().join("function-case.toml");
        fs::write(
            &case,
            "[meta]\nname='n'\nloadcase='l'\n[input]\nfile='input.toml'\n[setup]\nfile='setup.toml'\n\
             [mappings.simulation.knc]\nfile='knc-simulation-mappings.toml'\n\
             [mappings.simulation.knc.vertical]\nfile='knc-vertical-simulation-mappings.toml'\n\
             [mappings.measurement.knc.vertical]\nfile='knc-vertical-measurement-mappings.toml'\n",
        )
        .unwrap();

        let vertical = load_config(&case).unwrap().vertical.unwrap();
        assert_eq!(vertical.signals["front_left_travel"].entity, "fl");
        assert_eq!(vertical.signals["front_left_wheel_center_x"].component, "x");
        assert_eq!(
            vertical.measurement_signals["front_left_wheel_center_x"].entity,
            "lfwcx"
        );
    }

    #[test]
    fn missing_domain_mapping_file_names_the_reference() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("case.toml");
        fs::write(
            &p,
            "[input.svc.simulation]\nfile='simulation.json'\n\
             [mappings.simulation.svc]\nfile='mappings/missing.toml'\n",
        )
        .unwrap();
        let error = load_config(&p).err().unwrap().to_string();
        assert!(
            error.contains(&format!(
                "failed to read mapping file '{}'",
                d.path().join("mappings/missing.toml").display()
            )),
            "{error}"
        );
    }

    #[test]
    fn resolves_fully_flat_inline_mapping_headers() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("case.toml");
        fs::write(
            &p,
            "[meta]\nname='n'\nloadcase='l'\n[physics]\ngravity=9.8\n[input.svc.simulation]\nfile='simulation.json'\n\
             [mappings.simulation.svc.front_left_wheel_load]\nentity='e'\ncomponent='c'\nunit='kg'\n\
             [mappings.measurement.svc.front_left_mass_kg]\nentity='e2'\nunit='kg'\n",
        )
        .unwrap();

        let config = load_config(&p).unwrap();
        assert_eq!(config.signals["front_left_wheel_load"].entity, "e");
        assert_eq!(
            config.svc_measurement_signals["front_left_mass_kg"].entity,
            "e2"
        );
    }

    #[test]
    fn resolves_single_combined_mapping_file() {
        let d = tempfile::tempdir().unwrap();
        fs::write(
            d.path().join("combined-mappings.toml"),
            "[simulation.svc.front_left_wheel_load]\nentity='e'\ncomponent='c'\nunit='kg'\n\
             [measurement.svc.front_left_mass_kg]\nentity='e2'\nunit='kg'\n",
        )
        .unwrap();
        let p = d.path().join("case.toml");
        fs::write(
            &p,
            "[meta]\nname='n'\nloadcase='l'\n[physics]\ngravity=9.8\n[input.svc.simulation]\nfile='simulation.json'\n\
             [mappings]\nfile='combined-mappings.toml'\n",
        )
        .unwrap();

        let config = load_config(&p).unwrap();
        assert_eq!(config.signals["front_left_wheel_load"].entity, "e");
        assert_eq!(
            config.svc_measurement_signals["front_left_mass_kg"].entity,
            "e2"
        );
    }

    #[test]
    fn resolves_external_input_paths_relative_to_input_role_file() {
        let d = tempfile::tempdir().unwrap();
        fs::create_dir(d.path().join("input")).unwrap();
        fs::write(
            d.path().join("input/input.toml"),
            "[svc.simulation]\nfile='data/simulation.json'\n",
        )
        .unwrap();
        let p = d.path().join("case.toml");
        fs::write(
            &p,
            "[meta]\nname='n'\nloadcase='l'\n[input]\nfile='input/input.toml'\n[signals.simulation.svc.signals.total]\nentity='e'\ncomponent='c'\nunit='kg'\n[signals.measurement.svc.signals.total]\nentity='e'\nunit='kg'\n",
        )
        .unwrap();

        let config = load_config(&p).unwrap();
        assert_eq!(
            config.simulation_json,
            d.path().join("input/data/simulation.json")
        );
    }

    #[test]
    fn meta_rejects_redundant_domain() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("case.toml");
        fs::write(
            &p,
            "[meta]\nname='n'\ndomain='svc'\nloadcase='l'\n[input.svc.simulation]\ninput_file='simulation.json'\n[mappings.svc.total]\nentity='e'\ncomponent='c'\nunit='kg'\n",
        )
        .unwrap();
        assert!(load_config(&p).unwrap_err().to_string().contains("unknown"));
    }

    #[test]
    fn parses_canonical_inline_roles() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("case.toml");
        fs::write(&p, "[meta]\nname='n'\nloadcase='l'\n[physics]\ngravity=9.8\n[input.svc.simulation]\nfile='simulation.json'\n[signals.simulation.svc.signals.total]\nentity='e'\ncomponent='c'\nunit='kg'\n[signals.measurement.svc.signals.total]\nentity='e'\nunit='kg'\n[validation.svc.tolerances.absolute]\ntotal=1\n").unwrap();
        let config = load_config(&p).unwrap();
        assert_eq!(config.signals["total"].unit, "kg");
        assert_eq!(config.validation.unwrap().tolerances.absolute["total"], 1.0);
    }

    #[test]
    fn parses_canonical_toml_role_files_and_signal_inheritance() {
        let d = tempfile::tempdir().unwrap();
        fs::write(d.path().join("input.toml"), "[svc.simulation]\nfile='simulation.json'\n[knc.vertical.simulation]\nfile='simulation.h5'\n").unwrap();
        fs::write(d.path().join("setup.toml"), "[knc.vertical]\nbrakes_applied=false\nengine_running=false\nfront_arb_connected=true\n[knc.vertical.window]\nminimum_mm=-1\nmaximum_mm=1\nstep_mm=1\n").unwrap();
        fs::write(d.path().join("signals.toml"), "[simulation.svc.signals.total]\nentity='e'\ncomponent='c'\nunit='kg'\n[measurement.svc.signals.total]\nentity='e'\nunit='kg'\n[simulation.knc.signals.front_left_travel]\nentity='fl'\ncomponent='travel'\nunit='mm'\n[simulation.knc.vertical.signals.front_left_wheel_center_x]\nentity='fl'\ncomponent='x'\nunit='mm'\n[measurement.knc.vertical.signals.front_left_wheel_center_x]\nentity='fl'\nunit='mm'\n").unwrap();
        fs::write(
            d.path().join("validation.toml"),
            "[svc.validation.tolerances.absolute]\ntotal=1\n[knc.validation.tolerances.absolute]\nrear_left_wheel_center_x_mm=0.2\n[knc.vertical.tolerances.absolute]\nfront_left_wheel_center_x_mm=0.1\n",
        )
        .unwrap();
        let p = d.path().join("case.toml");
        fs::write(&p, "[meta]\nname='n'\nloadcase='l'\n[physics]\ngravity=9.8\n[input]\nfile='input.toml'\n[setup]\nfile='setup.toml'\n[signals]\nfile='signals.toml'\n[compare]\nfile='validation.toml'\n").unwrap();
        let config = load_config(&p).unwrap();
        assert_eq!(config.signals["total"].unit, "kg");
        assert_eq!(
            config.validation.as_ref().unwrap().tolerances.absolute["total"],
            1.0
        );
        let vertical = config.vertical.unwrap();
        assert_eq!(vertical.signals["front_left_travel"].component, "travel");
        assert_eq!(vertical.signals["front_left_wheel_center_x"].component, "x");
        assert_eq!(
            vertical.tolerances.absolute["front_left_wheel_center_x_mm"],
            0.1
        );
        assert_eq!(
            vertical.tolerances.absolute["rear_left_wheel_center_x_mm"],
            0.2
        );
    }

    #[test]
    fn rejects_ambiguous_compare_and_validation_roles() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("case.toml");
        fs::write(
            &p,
            "[compare]\nfile='tolerances.toml'\n[validation]\nfile='other.toml'\n",
        )
        .unwrap();
        assert!(
            load_config_with_input(&p, d.path().join("input.h5"), Some("vertical"))
                .unwrap_err()
                .to_string()
                .contains("[compare] cannot be combined with [validation]")
        );
    }

    #[test]
    fn merges_knc_defaults_with_vertical_setup_and_tolerances() {
        let d = tempfile::tempdir().unwrap();
        fs::write(
            d.path().join("input.toml"),
            "[knc.vertical.simulation]\nfile='simulation.h5'\n",
        )
        .unwrap();
        fs::write(
            d.path().join("setup.toml"),
            "[knc]\nbrakes_applied=true\nengine_running=false\nfront_arb_connected=true\nrear_arb_connected=true\n[knc.vertical.window]\nminimum_mm=-1\nmaximum_mm=1\nstep_mm=1\n",
        )
        .unwrap();
        fs::write(
            d.path().join("signals.toml"),
            "[simulation.knc.vertical.signals.front_left_travel]\nentity='fl'\ncomponent='travel'\nunit='mm'\n[measurement.knc.vertical.signals.front_left_travel]\nentity='fl'\nunit='mm'\n",
        )
        .unwrap();
        fs::write(
            d.path().join("validation.toml"),
            "[knc.validation.tolerances.absolute]\nfront_left_wheel_center_x_mm=0.1\nrear_left_wheel_center_x_mm=0.1\n[knc.vertical.validation.tolerances.absolute]\nfront_left_wheel_center_x_mm=0.05\n[knc.vertical.validation.tolerances.relative_percent]\nrear_left_wheel_center_x_mm=2\n",
        )
        .unwrap();
        let p = d.path().join("case.toml");
        fs::write(
            &p,
            "[meta]\nname='n'\nloadcase='l'\n[input]\nfile='input.toml'\n[setup]\nfile='setup.toml'\n[signals]\nfile='signals.toml'\n[validation]\nfile='validation.toml'\n",
        )
        .unwrap();

        let vertical = load_config(&p).unwrap().vertical.unwrap();

        assert!(vertical.setup.brakes_applied);
        assert_eq!(
            vertical.tolerances.absolute["front_left_wheel_center_x_mm"],
            0.05
        );
        assert_eq!(
            vertical.tolerances.absolute["rear_left_wheel_center_x_mm"],
            0.1
        );
        assert_eq!(
            vertical.tolerances.relative_percent["rear_left_wheel_center_x_mm"],
            2.0
        );
    }

    #[test]
    fn vertical_meta_window_and_metric_override_knc_level() {
        let d = tempfile::tempdir().unwrap();
        fs::write(
            d.path().join("input.toml"),
            "[knc.vertical.simulation]\nfile='simulation.h5'\n",
        )
        .unwrap();
        fs::write(
            d.path().join("setup.toml"),
            "[knc.meta]\nbrakes_applied=true\nengine_running=false\nfront_arb_connected=true\nrear_arb_connected=true\n\
             [knc.window]\nminimum_mm=-80\nmaximum_mm=80\nstep_mm=1\nplot_branches='selected'\n\
             [knc.metric]\ngradient_points_mm=10\ndirection='fall'\nindex=3\n\
             [knc.vertical.window]\nminimum_mm=-60\nmaximum_mm=60\n\
             [knc.vertical.metric]\ngradient_points_mm=5\n\
             [knc.vertical.metric.measurement]\nindex=1\n",
        )
        .unwrap();
        fs::write(
            d.path().join("signals.toml"),
            "[simulation.knc.vertical.signals.front_left_travel]\nentity='fl'\ncomponent='travel'\nunit='mm'\n[measurement.knc.vertical.signals.front_left_travel]\nentity='fl'\nunit='mm'\n",
        )
        .unwrap();
        let p = d.path().join("case.toml");
        fs::write(
            &p,
            "[meta]\nname='n'\nloadcase='l'\n[input]\nfile='input.toml'\n[setup]\nfile='setup.toml'\n[signals]\nfile='signals.toml'\n",
        )
        .unwrap();

        let vertical = load_config(&p).unwrap().vertical.unwrap();

        assert!(vertical.setup.brakes_applied);
        assert_eq!(vertical.setup.rear_arb_connected, Some(true));
        assert_eq!(
            vertical.setup.summary(),
            "brakes applied: yes | engine running: no | front ARB connected: yes | rear ARB connected: yes"
        );
        // Vertical overrides per key; unset keys inherit from [knc].
        assert_eq!(
            (
                vertical.window.minimum_mm,
                vertical.window.maximum_mm,
                vertical.window.step_mm
            ),
            (-60.0, 60.0, 1.0)
        );
        assert_eq!(vertical.window.gradient_points_mm, Some(5.0));
        let (measurement, simulation) = (&vertical.branch.measurement, &vertical.branch.simulation);
        assert_eq!((measurement.index, simulation.index), (1, 3));
        assert_eq!(measurement.direction, "fall");
        assert_eq!(simulation.plot_branches, "selected");
        assert!(measurement.defaulted.is_empty() && simulation.defaulted.is_empty());
    }

    #[test]
    fn vertical_reports_defaulted_branch_keys() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("case.toml");
        let base = "[case]\nname='n'\nloadcase='l'\n[vertical.inputs]\nmeasurement_hdf5='measurement.h5'\nsimulation_hdf5='simulation.h5'\n[vertical.setup]\nbrakes_applied=true\nengine_running=false\nfront_arb_connected=true\n[vertical.window]\nminimum_mm=-10\nmaximum_mm=10\nstep_mm=1\n[vertical.signals.x]\nentity='e'\ncomponent='c'\nunit='mm'\n[vertical.measurement_signals.x]\nentity='e'\nunit='mm'\n";
        fs::write(&p, base).unwrap();
        let branch = load_config(&p).unwrap().vertical.unwrap().branch;
        assert_eq!(
            branch.simulation.defaulted,
            ["direction", "index", "plot_branches"]
        );
        fs::write(&p, format!("{base}[vertical.metric]\nindex=1\n")).unwrap();
        let branch = load_config(&p).unwrap().vertical.unwrap().branch;
        assert_eq!(branch.simulation.defaulted, ["direction", "plot_branches"]);
        // A key set only for one source kind stays defaulted for the other.
        fs::write(
            &p,
            format!("{base}[vertical.metric.measurement]\nindex=1\n"),
        )
        .unwrap();
        let branch = load_config(&p).unwrap().vertical.unwrap().branch;
        assert!(!branch.measurement.defaulted.contains(&"index"));
        assert!(branch.simulation.defaulted.contains(&"index"));
        for invalid in [
            "[vertical.metric]\nplot_branches='all'\n",
            "[vertical.metric]\nindex=0\n",
            "[vertical.metric.simulation]\ngradient_points_mm=1\n",
        ] {
            fs::write(&p, format!("{base}{invalid}")).unwrap();
            assert!(load_config(&p).is_err(), "accepted {invalid}");
        }
    }

    #[test]
    fn rejects_vertical_role_with_mismatched_hdf5_source_kind() {
        let d = tempfile::tempdir().unwrap();
        let input = d.path().join("input.h5");
        source_kind(&input, "measurement");
        let config = d.path().join("case.toml");
        fs::write(
            &config,
            format!(
                "[meta]\nname='n'\nloadcase='l'\n[input.knc.vertical.simulation]\nfile='{}'\n",
                input.display()
            ),
        )
        .unwrap();

        assert!(
            load_config(&config)
                .unwrap_err()
                .to_string()
                .contains("source_kind must be simulation")
        );
    }

    #[test]
    fn rejects_input_whose_declared_domain_or_test_differs() {
        let d = tempfile::tempdir().unwrap();
        let config = d.path().join("case.toml");
        for (attrs, expected) in [
            (&[("domain", "svc")][..], "has domain 'svc', expected 'knc'"),
            (
                &[("domain", "knc"), ("test", "static")][..],
                "has test 'static', expected 'vertical'",
            ),
        ] {
            let input = d.path().join("input.h5");
            source_kind(&input, "simulation");
            let file = File::open_rw(&input).unwrap();
            for (name, value) in attrs {
                string(&file, name, value);
            }
            file.close().unwrap();
            fs::write(
                &config,
                format!(
                    "[meta]\nname='n'\nloadcase='l'\n[input.knc.vertical.simulation]\nfile='{}'\n",
                    input.display()
                ),
            )
            .unwrap();

            let message = load_config(&config).unwrap_err().to_string();
            assert!(message.contains(expected), "{message}");
            fs::remove_file(&input).unwrap();
        }
        // Files without domain and test (converted earlier) are accepted.
        let input = d.path().join("input.h5");
        source_kind(&input, "simulation");
        fs::write(
            &config,
            format!(
                "[meta]\nname='n'\nloadcase='l'\n[input.knc.vertical.simulation]\nfile='{}'\n",
                input.display()
            ),
        )
        .unwrap();
        assert!(
            !load_config(&config)
                .err()
                .is_some_and(|error| error.to_string().contains("expected"))
        );
    }

    #[test]
    fn derives_direct_input_kinds_and_rejects_mixed_domains() {
        let d = tempfile::tempdir().unwrap();
        let svc = d.path().join("svc.h5");
        let vertical = d.path().join("vertical.h5");
        source_kind(&svc, "measurement");
        source_kind(&vertical, "simulation");
        let config = d.path().join("case.toml");
        fs::write(
            &config,
            format!(
                "[meta]\nname='n'\nloadcase='l'\n[input.svc]\nfile={:?}\n[input.knc.vertical]\nfile={:?}\n[setup.knc.vertical]\nbrakes_applied=true\nengine_running=false\nfront_arb_connected=true\n[setup.knc.vertical.window]\nminimum_mm=-1\nmaximum_mm=1\nstep_mm=1\n[signals.simulation.svc.signals.x]\nentity='e'\ncomponent='c'\nunit='kg'\n[signals.measurement.svc.signals.x]\nentity='e'\nunit='kg'\n[signals.simulation.knc.vertical.signals.x]\nentity='e'\ncomponent='c'\nunit='mm'\n[signals.measurement.knc.vertical.signals.x]\nentity='e'\nunit='mm'\n",
                svc, vertical
            ),
        )
        .unwrap();
        assert!(
            load_config(&config)
                .unwrap_err()
                .to_string()
                .contains("mixes simulation and measurement")
        );
    }

    #[test]
    fn resolves_static_baseline_relative_to_run() {
        let d = tempfile::tempdir().unwrap();
        let run_dir = d.path().join("run");
        fs::create_dir(&run_dir).unwrap();
        let input = run_dir.join("vertical.h5");
        let static_input = run_dir.join("static.h5");
        source_kind(&input, "simulation");
        source_kind(&static_input, "simulation");
        let config = d.path().join("case.toml");
        fs::write(&config, "[meta]\nname='n'\nloadcase='l'\n[setup.knc.vertical]\nbrakes_applied=true\nengine_running=false\nfront_arb_connected=true\n[setup.knc.vertical.window]\nminimum_mm=-1\nmaximum_mm=1\nstep_mm=1\n[signals.simulation.knc.vertical.signals.x]\nentity='e'\ncomponent='c'\nunit='mm'\n[signals.measurement.knc.vertical.signals.x]\nentity='e'\nunit='mm'\n").unwrap();
        let run = run_dir.join("run.toml");
        fs::write(
            &run,
            "[input.knc.static]\nfile='static.h5'\n[input.knc.vertical]\nfile='vertical.h5'\n",
        )
        .unwrap();
        let vertical = load_config_and_run(&config, &run)
            .unwrap()
            .vertical
            .unwrap();
        assert_eq!(vertical.simulation_hdf5, input);
        assert_eq!(vertical.static_simulation_hdf5, Some(static_input));
    }

    #[test]
    fn run_metadata_overrides_shared_configuration_for_each_run() {
        let directory = tempfile::tempdir().unwrap();
        let config = directory.path().join("config.toml");
        let first_input = directory.path().join("first.h5");
        let second_input = directory.path().join("second.h5");
        source_kind(&first_input, "simulation");
        source_kind(&second_input, "simulation");
        fs::write(
            &config,
            "[meta]\nname='shared'\nloadcase='shared'\n[physics]\ngravity=9.80665\n[signals.simulation.svc.signals.total]\nentity='vehicle'\ncomponent='mass'\nunit='kg'\n[signals.measurement.svc.signals.total]\nentity='vehicle'\nunit='kg'\n",
        )
        .unwrap();
        let first_run = directory.path().join("first.toml");
        fs::write(
            &first_run,
            format!(
                "[meta]\nname='first run'\nloadcase='curb'\n[input.svc]\nfile={:?}\n",
                first_input
            ),
        )
        .unwrap();
        let second_run = directory.path().join("second.toml");
        fs::write(
            &second_run,
            format!(
                "[meta]\nname='second run'\nloadcase='two-up'\n[input.svc]\nfile={:?}\n",
                second_input
            ),
        )
        .unwrap();
        let undefined_run = directory.path().join("undefined.toml");
        fs::write(
            &undefined_run,
            format!("[input.svc]\nfile={first_input:?}\n"),
        )
        .unwrap();

        let first = load_config_and_run(&config, &first_run).unwrap();
        let second = load_config_and_run(&config, &second_run).unwrap();
        let undefined = load_config_and_run(&config, &undefined_run).unwrap();

        assert_eq!(
            (first.name.as_str(), first.loadcase.as_str()),
            ("first run", "curb")
        );
        assert_eq!(
            (second.name.as_str(), second.loadcase.as_str()),
            ("second run", "two-up")
        );
        assert_eq!(first.simulation_json, absolute(&first_input).unwrap());
        assert_eq!(second.simulation_json, absolute(&second_input).unwrap());
        assert_eq!(undefined.name, "undefined");
        assert_eq!(undefined.loadcase, "undefined");
    }

    #[test]
    fn direct_input_reports_undefined_metadata_when_absent() {
        let directory = tempfile::tempdir().unwrap();
        let input = directory.path().join("input.h5");
        source_kind(&input, "simulation");
        let config = directory.path().join("config.toml");
        fs::write(
            &config,
            "[physics]\ngravity=9.80665\n[signals.simulation.svc.signals.total]\nentity='vehicle'\ncomponent='mass'\nunit='kg'\n[signals.measurement.svc.signals.total]\nentity='vehicle'\nunit='kg'\n",
        )
        .unwrap();

        let loaded = load_config_with_input(&config, &input, None).unwrap();

        assert_eq!(loaded.name, "undefined");
        assert_eq!(loaded.loadcase, "undefined");

        fs::write(
            &config,
            "[meta]\nname='named'\n[physics]\ngravity=9.80665\n[signals.simulation.svc.signals.total]\nentity='vehicle'\ncomponent='mass'\nunit='kg'\n[signals.measurement.svc.signals.total]\nentity='vehicle'\nunit='kg'\n",
        )
        .unwrap();
        let loaded = load_config_with_input(&config, &input, None).unwrap();
        assert_eq!(loaded.name, "named");
        assert_eq!(loaded.loadcase, "undefined");
    }

    #[test]
    fn rejects_static_baseline_with_measurement_source_kind() {
        let d = tempfile::tempdir().unwrap();
        let input = d.path().join("vertical.h5");
        let static_input = d.path().join("static.h5");
        source_kind(&input, "simulation");
        source_kind(&static_input, "measurement");
        let config = d.path().join("case.toml");
        fs::write(
            &config,
            format!(
                "[meta]\nname='n'\nloadcase='l'\n[input.knc.static]\nfile={static_input:?}\n[input.knc.vertical]\nfile={input:?}\n[setup.knc.vertical]\nbrakes_applied=true\nengine_running=false\nfront_arb_connected=true\n[setup.knc.vertical.window]\nminimum_mm=-1\nmaximum_mm=1\nstep_mm=1\n[signals.simulation.knc.vertical.signals.x]\nentity='e'\ncomponent='c'\nunit='mm'\n"
            ),
        )
        .unwrap();
        assert!(
            load_config(&config)
                .unwrap_err()
                .to_string()
                .contains("source_kind must be simulation")
        );
    }

    #[test]
    fn rejects_static_baseline_with_measurement_vertical_input() {
        let d = tempfile::tempdir().unwrap();
        let input = d.path().join("vertical.h5");
        let static_input = d.path().join("static.h5");
        source_kind(&input, "measurement");
        source_kind(&static_input, "simulation");
        let config = d.path().join("case.toml");
        fs::write(
            &config,
            format!(
                "[meta]\nname='n'\nloadcase='l'\n[input.knc.static]\nfile={static_input:?}\n[input.knc.vertical]\nfile={input:?}\n[setup.knc.vertical]\nbrakes_applied=true\nengine_running=false\nfront_arb_connected=true\n[setup.knc.vertical.window]\nminimum_mm=-1\nmaximum_mm=1\nstep_mm=1\n[signals.simulation.knc.vertical.signals.x]\nentity='e'\ncomponent='c'\nunit='mm'\n"
            ),
        )
        .unwrap();
        assert!(
            load_config(&config)
                .unwrap_err()
                .to_string()
                .contains("requires simulation [input.knc.vertical]")
        );
    }

    #[test]
    fn rejects_svc_json_without_source_kind() {
        let d = tempfile::tempdir().unwrap();
        let input = d.path().join("svc.json");
        fs::write(&input, "{}").unwrap();
        let config = d.path().join("case.toml");
        fs::write(
            &config,
            format!(
                "[meta]\nname='n'\nloadcase='l'\n[input.svc]\nfile={input:?}\n[signals.simulation.svc.signals.x]\nentity='e'\ncomponent='c'\nunit='kg'\n"
            ),
        )
        .unwrap();
        assert!(
            load_config(&config)
                .unwrap_err()
                .to_string()
                .contains("missing required source_kind")
        );
    }

    #[test]
    fn direct_vertical_input_keeps_configured_static_simulation() {
        let d = tempfile::tempdir().unwrap();
        let simulation = d.path().join("simulation.h5");
        source_kind(&simulation, "simulation");
        let config = d.path().join("case.toml");
        fs::write(
            &config,
            format!(
                "[meta]\nname='n'\nloadcase='l'\n\
                 [setup.knc.vertical]\nbrakes_applied=true\nengine_running=false\nfront_arb_connected=true\n\
                 [setup.knc.vertical.window]\nminimum_mm=-1\nmaximum_mm=1\nstep_mm=1\n\
                 [signals.simulation.knc.vertical.signals.x]\nentity='e'\ncomponent='c'\nunit='mm'\n\
                 [signals.measurement.knc.vertical.signals.x]\nentity='e'\nunit='mm'\n\
                 [input.knc.vertical.static_simulation]\nfile='{}'\n",
                simulation.display(),
            ),
        )
        .unwrap();

        let loaded = load_config_with_input(&config, &simulation, Some("vertical")).unwrap();
        assert_eq!(
            loaded
                .vertical
                .unwrap()
                .static_simulation_hdf5
                .unwrap()
                .canonicalize()
                .unwrap(),
            simulation.canonicalize().unwrap()
        );
    }

    #[test]
    fn example_templates_load_before_generated_outputs_exist() {
        let examples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");

        assert!(
            load_config_and_run(
                examples.join("config.toml"),
                examples.join("input/run-simulation.toml")
            )
            .is_ok()
        );
    }
}
