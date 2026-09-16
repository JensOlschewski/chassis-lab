use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use clap::{Args, ValueEnum};
use sha2::{Digest, Sha256};

#[derive(Debug, Args)]
pub(crate) struct ConvertArgs {
    /// SVC RES, vertical RES, ACD, or vehicle-information HTML input.
    input: PathBuf,
    /// Output file. Vehicle-information conversion defaults to a JSON file
    /// beside the input; an output directory retains the input filename.
    #[arg(short, long, value_name = "OUTPUT")]
    output: Option<PathBuf>,
    /// Replace an existing output file.
    #[arg(long)]
    force: bool,
    /// MOI sequence paired with veh_inf.htm. Defaults to MOISequence_S7_R2.txt
    /// beside the HTML input when present.
    #[arg(long, value_name = "MOI_FILE")]
    moi: Option<PathBuf>,
    /// Loadcase to convert from vehicle-information input.
    #[arg(long, value_enum, ignore_case = true, value_name = "LOADCASE")]
    loadcase: Option<VehicleLoadcase>,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum VehicleLoadcase {
    #[value(name = "CURB")]
    Curb,
    #[value(name = "2UP")]
    TwoUp,
}

#[derive(Clone, Copy)]
enum InputKind {
    SvcRes,
    Res,
    Acd,
    VehicleInfo,
}

impl InputKind {
    fn from_path(path: &Path) -> Result<Self> {
        if !path.is_file() {
            bail!(
                "input file does not exist or is not a file: {}",
                path.display()
            );
        }
        let extension = path.extension().ok_or_else(|| {
            anyhow::anyhow!("input file has no extension; supported extensions are .res and .acd")
        })?;
        let extension = extension.to_str().ok_or_else(|| {
            anyhow::anyhow!("input file extension is not valid UTF-8; expected .res or .acd")
        })?;
        if extension.eq_ignore_ascii_case("res") {
            let stem = path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or_default();
            let stem = stem.to_ascii_lowercase();
            if stem == "svc" || stem.ends_with("_svc") || stem.ends_with("-svc") {
                Ok(Self::SvcRes)
            } else {
                Ok(Self::Res)
            }
        } else if extension.eq_ignore_ascii_case("acd") {
            Ok(Self::Acd)
        } else if extension.eq_ignore_ascii_case("htm") || extension.eq_ignore_ascii_case("html") {
            Ok(Self::VehicleInfo)
        } else {
            bail!("unsupported input extension '.{extension}'; expected .res, .acd, or .htm");
        }
    }

    fn default_output(self, input: &Path) -> PathBuf {
        match self {
            Self::SvcRes => input.with_extension("json"),
            Self::Res => input.with_extension("simulation.h5"),
            Self::Acd => input.with_extension("measurement.h5"),
            Self::VehicleInfo => input
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .to_path_buf(),
        }
    }

    fn output_type(self) -> &'static str {
        match self {
            Self::SvcRes => "SVC JSON",
            Self::Res => "simulation HDF5",
            Self::Acd => "measurement HDF5",
            Self::VehicleInfo => "SVC reference JSON",
        }
    }
}

pub(crate) fn convert(args: ConvertArgs) -> Result<()> {
    let kind = InputKind::from_path(&args.input)?;
    if matches!(kind, InputKind::VehicleInfo) {
        let loadcase = args.loadcase.ok_or_else(|| {
            anyhow::anyhow!("vehicle-information conversion requires --loadcase CURB or 2UP")
        })?;
        let output = vehicle_output(&args.input, args.output.as_deref())?;
        validate_output(&args.input, &output, args.force)?;
        let moi = args.moi.as_deref().map(Path::to_path_buf).or_else(|| {
            let path = args.input.parent()?.join("MOISequence_S7_R2.txt");
            path.is_file().then_some(path)
        });
        convert_vehicle_reference(&args.input, moi.as_deref(), loadcase, &output, args.force)?;
        println!("generated {} ({})", output.display(), kind.output_type());
        return Ok(());
    }
    let output = args
        .output
        .unwrap_or_else(|| kind.default_output(&args.input));
    if matches!(kind, InputKind::SvcRes)
        && !output
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("json"))
    {
        bail!(
            "SVC reference output must use .json extension: {}",
            output.display()
        );
    }
    validate_output(&args.input, &output, args.force)?;

    match kind {
        InputKind::SvcRes => clab_converter::convert_svc_res(&args.input, &output, args.force),
        InputKind::Res => clab_converter::convert_res(&args.input, &output, args.force),
        InputKind::Acd => {
            clab_converter::measurement_hdf5::import_acd(&args.input, &output, args.force)
        }
        InputKind::VehicleInfo => unreachable!(),
    }
    .with_context(|| format!("failed to generate '{}'", output.display()))?;

    println!("generated {} ({})", output.display(), kind.output_type());
    Ok(())
}

fn vehicle_output(input: &Path, output: Option<&Path>) -> Result<PathBuf> {
    let filename = input
        .file_stem()
        .filter(|stem| !stem.is_empty())
        .map(|stem| PathBuf::from(stem).with_extension("json"))
        .ok_or_else(|| anyhow::anyhow!("vehicle-information input has no filename"))?;
    match output {
        Some(output) if output.is_dir() => Ok(output.join(filename)),
        Some(output)
            if output
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extension.eq_ignore_ascii_case("json")) =>
        {
            Ok(output.to_path_buf())
        }
        Some(output) => bail!(
            "vehicle-information output must be an existing directory or a .json file: {}",
            output.display()
        ),
        None => Ok(input
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(filename)),
    }
}

fn convert_vehicle_reference(
    input: &Path,
    moi: Option<&Path>,
    loadcase: VehicleLoadcase,
    output: &Path,
    force: bool,
) -> Result<()> {
    let (mut curb, two_up) = parse_vehicle_html(&read_text(input)?)?;
    let html_source = source(input, "vehicle information")?;
    match loadcase {
        VehicleLoadcase::Curb => {
            let mut sources = vec![html_source];
            if let Some(moi) = moi {
                curb.extend(parse_moi_values(&read_text(moi)?)?);
                sources.push(source(moi, "MOI CG height and inertia")?);
            }
            write_reference(output, sources, curb, force)
        }
        VehicleLoadcase::TwoUp => write_reference(output, vec![html_source], two_up, force),
    }
}

fn write_reference(
    output: &Path,
    sources: Vec<serde_json::Value>,
    values: BTreeMap<String, f64>,
    force: bool,
) -> Result<()> {
    if output.exists() && !force {
        bail!(
            "output file already exists: {}; pass --force to replace it",
            output.display()
        );
    }
    let document = serde_json::json!({
        "schema_version": 1,
        "coordinate_system": {"cg_x_origin": "front_axle_wheel_center", "cg_x_positive": "rearward"},
        "sources": sources,
        "values": values,
    });
    clab_converter::write_atomically(output, force, |temporary| {
        fs::write(temporary, serde_json::to_string_pretty(&document)? + "\n")?;
        Ok(())
    })
}

fn source(path: &Path, result: &str) -> Result<serde_json::Value> {
    Ok(
        serde_json::json!({"file": path.file_name().and_then(|name| name.to_str()).unwrap_or("source"), "sha256": format!("{:x}", Sha256::digest(fs::read(path)?)), "result": result}),
    )
}

fn read_text(path: &Path) -> Result<String> {
    let bytes = fs::read(path)?;
    if bytes.starts_with(&[0xff, 0xfe]) {
        let (units, remainder) = bytes[2..].as_chunks::<2>();
        if !remainder.is_empty() {
            bail!("invalid UTF-16LE text: {}", path.display());
        }
        let units = units
            .iter()
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect::<Vec<_>>();
        return String::from_utf16(&units)
            .map_err(|_| anyhow::anyhow!("invalid UTF-16LE text: {}", path.display()));
    }
    String::from_utf8(bytes)
        .map_err(|_| anyhow::anyhow!("input is not valid UTF-8: {}", path.display()))
}

fn parse_vehicle_html(input: &str) -> Result<(BTreeMap<String, f64>, BTreeMap<String, f64>)> {
    let rows = html_rows(input);
    let track = row_value(&rows, "track front/rear", 1)?;
    let (front_track, rear_track) = track
        .split_once('/')
        .ok_or_else(|| anyhow::anyhow!("Track Front/Rear must contain both values"))?;
    let wheelbase = number(row_value(&rows, "wheelbase", 1)?)?;
    let mut curb = fixed_values(number(front_track)?, number(rear_track)?, wheelbase);
    let mut two_up = curb.clone();
    for (label, metric) in [
        ("left front", "front_left_mass_kg"),
        ("right front", "front_right_mass_kg"),
        ("left rear", "rear_left_mass_kg"),
        ("right rear", "rear_right_mass_kg"),
    ] {
        let row = row(&rows, label)?;
        curb.insert(metric.into(), number(&row[3])?);
        two_up.insert(metric.into(), number(&row[4])?);
    }
    complete_masses(&mut curb);
    complete_masses(&mut two_up);
    Ok((curb, two_up))
}

fn fixed_values(front_track: f64, rear_track: f64, wheelbase: f64) -> BTreeMap<String, f64> {
    BTreeMap::from([
        ("front_track_mm".into(), front_track),
        ("rear_track_mm".into(), rear_track),
        ("wheelbase_mm".into(), wheelbase),
    ])
}

fn complete_masses(values: &mut BTreeMap<String, f64>) {
    let front = values["front_left_mass_kg"] + values["front_right_mass_kg"];
    let rear = values["rear_left_mass_kg"] + values["rear_right_mass_kg"];
    let total = front + rear;
    values.extend([
        ("front_mass_kg".into(), front),
        ("rear_mass_kg".into(), rear),
        ("total_mass_kg".into(), total),
        ("front_distribution_percent".into(), front / total * 100.0),
        ("rear_distribution_percent".into(), rear / total * 100.0),
        (
            "cg_x_from_front_axle_mm".into(),
            rear / total * values["wheelbase_mm"],
        ),
    ]);
}

fn html_rows(input: &str) -> Vec<Vec<String>> {
    let lower = input.to_ascii_lowercase();
    let mut rows = Vec::new();
    let mut start = 0;
    while let Some(relative) = lower[start..].find("<tr") {
        let row_start = start + relative;
        let Some(end_relative) = lower[row_start..].find("</tr>") else {
            break;
        };
        let row = &input[row_start..row_start + end_relative];
        let mut cells = Vec::new();
        let row_lower = row.to_ascii_lowercase();
        let mut cell_start = 0;
        while let Some(relative) = row_lower[cell_start..].find("<td") {
            let content_start = cell_start + relative;
            let Some(tag_end) = row_lower[content_start..].find('>') else {
                break;
            };
            let content_start = content_start + tag_end + 1;
            let Some(content_end) = row_lower[content_start..].find("</td>") else {
                break;
            };
            cells.push(strip_html(&row[content_start..content_start + content_end]));
            cell_start = content_start + content_end + 5;
        }
        if !cells.is_empty() {
            rows.push(cells);
        }
        start = row_start + end_relative + 5;
    }
    rows
}

fn strip_html(input: &str) -> String {
    let mut text = String::new();
    let mut in_tag = false;
    for character in input.chars() {
        match character {
            '<' => in_tag = true,
            '>' => {
                in_tag = false;
                text.push(' ');
            }
            _ if !in_tag => text.push(character),
            _ => {}
        }
    }
    text.replace("&nbsp;", " ")
        .replace("&nbsp", " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn row<'a>(rows: &'a [Vec<String>], label: &str) -> Result<&'a Vec<String>> {
    rows.iter()
        .find(|row| {
            row.first()
                .is_some_and(|value| value.trim_end_matches(':').eq_ignore_ascii_case(label))
                || row
                    .get(2)
                    .is_some_and(|value| value.trim_end_matches(':').eq_ignore_ascii_case(label))
        })
        .filter(|row| row.len() >= 5)
        .ok_or_else(|| anyhow::anyhow!("vehicle HTML missing '{label}' row"))
}

fn row_value<'a>(rows: &'a [Vec<String>], label: &str, index: usize) -> Result<&'a str> {
    row(rows, label)?
        .get(index)
        .map(String::as_str)
        .ok_or_else(|| anyhow::anyhow!("vehicle HTML missing '{label}' value"))
}

fn number(value: &str) -> Result<f64> {
    value
        .trim()
        .parse()
        .map_err(|_| anyhow::anyhow!("expected numeric vehicle value, got '{value}'"))
}

fn parse_moi_values(input: &str) -> Result<BTreeMap<String, f64>> {
    if !input.contains("Inertia Units=kgm²") {
        bail!("MOI sequence inertia units must be kgm²");
    }
    let vehicle = input
        .split_once("Body Inertia Results:Complete Vehicle")
        .map(|(_, vehicle)| vehicle)
        .ok_or_else(|| anyhow::anyhow!("MOI sequence missing complete-vehicle inertia"))?;
    let vehicle = vehicle
        .split_once("Body Inertia Results:Sprung Mass")
        .map(|(vehicle, _)| vehicle)
        .unwrap_or(vehicle);
    Ok(BTreeMap::from([
        ("cg_height_mm".into(), moi_number(input, "CG Ground Z=")?),
        (
            "inertia_xx_kg_mm2".into(),
            moi_number(vehicle, "Ixx Ground=")? * 1_000_000.0,
        ),
        (
            "inertia_yy_kg_mm2".into(),
            moi_number(vehicle, "Iyy Ground=")? * 1_000_000.0,
        ),
        (
            "inertia_zz_kg_mm2".into(),
            moi_number(vehicle, "Izz Ground=")? * 1_000_000.0,
        ),
    ]))
}

fn moi_number(input: &str, label: &str) -> Result<f64> {
    input
        .lines()
        .find_map(|line| line.trim().strip_prefix(label))
        .map(number)
        .transpose()?
        .ok_or_else(|| anyhow::anyhow!("MOI sequence missing {label}"))
}

fn validate_output(input: &Path, output: &Path, force: bool) -> Result<()> {
    if output.exists() && std::fs::canonicalize(input)? == std::fs::canonicalize(output)? {
        bail!(
            "output path must not be the same as input: {}",
            output.display()
        );
    }
    let parent = output
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    if !parent.is_dir() {
        bail!(
            "output parent directory does not exist: {}",
            parent.display()
        );
    }
    if output.is_dir() {
        bail!("output path is a directory: {}", output.display());
    }
    if output.exists() && !force {
        bail!(
            "output file already exists: {}; pass --force to replace it",
            output.display()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{parse_moi_values, parse_vehicle_html};
    use crate::Cli;
    use clap::Parser;

    #[test]
    fn parses_convert_command() {
        let cli = Cli::try_parse_from(["clab", "convert", "input.res", "-o", "output.h5"]).unwrap();
        assert!(format!("{cli:?}").contains("Convert"));
    }

    #[test]
    fn parses_curb_and_2up_vehicle_values() {
        let html = "<tr><td>Track Front/Rear:</td><td>1498 / 1463</td><td>Left Front:</td><td>413</td><td>449</td></tr><tr><td>Wheelbase:</td><td>2497</td><td>Right Front:</td><td>389</td><td>427</td></tr><tr><td>Hip Point:</td><td></td><td>Left Rear:</td><td>347</td><td>384</td></tr><tr><td>Other:</td><td></td><td>Right Rear:</td><td>351</td><td>383</td></tr>";
        let (curb, two_up) = parse_vehicle_html(html).unwrap();
        assert_eq!(curb["total_mass_kg"], 1500.0);
        assert_eq!(two_up["total_mass_kg"], 1643.0);
        assert_eq!(two_up["front_track_mm"], 1498.0);
        assert_eq!(two_up["cg_x_from_front_axle_mm"], 1165.6719415702983);
        let moi = parse_moi_values("Inertia Units=kgm²\nBody Inertia Results:Complete Vehicle\nIxx Ground=508.4\nIyy Ground=1915.1\nIzz Ground=2014.7\nBody Inertia Results:Sprung Mass\nCG Ground Z=489.2\n").unwrap();
        assert_eq!(moi["cg_height_mm"], 489.2);
        assert_eq!(moi["inertia_xx_kg_mm2"], 508_400_000.0);
    }
}
