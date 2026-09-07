use std::collections::{HashMap, HashSet};
use std::path::Path;

use encoding_rs::WINDOWS_1252;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq)]
pub struct AcdMeasurement {
    pub encoding: String,
    pub dialect: String,
    pub channels: Vec<AcdChannel>,
    pub test_info: Vec<AcdTestInfo>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AcdChannel {
    pub name: String,
    pub description: String,
    pub unit: String,
    pub offset: f64,
    pub column: u64,
    pub values: Vec<f64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcdTestInfo {
    pub name: String,
    pub description: String,
    pub data_type: String,
    pub unit: String,
    pub value: String,
}

#[derive(Debug, Error)]
pub enum AcdError {
    #[error("failed to read ACD file '{path}': {source}")]
    Read {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("ACD format error: {0}")]
    Format(String),
}

pub fn parse_file(path: impl AsRef<Path>) -> Result<AcdMeasurement, AcdError> {
    let path = path.as_ref();
    let bytes = std::fs::read(path).map_err(|source| AcdError::Read {
        path: path.display().to_string(),
        source,
    })?;
    parse_bytes(&bytes)
}

pub fn parse_bytes(bytes: &[u8]) -> Result<AcdMeasurement, AcdError> {
    let (text, encoding) = match std::str::from_utf8(bytes) {
        Ok(text) => (
            text.strip_prefix('\u{feff}').unwrap_or(text).to_owned(),
            "utf-8-sig",
        ),
        Err(_) => {
            let (text, _, _) = WINDOWS_1252.decode(bytes);
            (text.into_owned(), "cp1252")
        }
    };
    parse_text(&text, encoding)
}

fn parse_text(text: &str, encoding: &str) -> Result<AcdMeasurement, AcdError> {
    let lines: Vec<_> = text.lines().collect();
    let mut metadata = HashMap::new();
    let mut test_info = Vec::new();
    let mut first_content = None;
    let mut kpoly_line = None;
    let mut data_start = None;

    for (index, line) in lines.iter().enumerate() {
        let line_number = index + 1;
        if first_content.is_none() && !line.trim().is_empty() {
            first_content = Some(line.trim());
        }
        let fields = fields(line);
        if fields.len() >= 7 && fields[0] == "TestInfo:" {
            test_info.push(AcdTestInfo {
                name: fields[1].to_owned(),
                description: fields[3].to_owned(),
                data_type: fields[4].to_owned(),
                unit: normalize_unit(fields[5]),
                value: normalize_unit(&fields[6..].join("\t")),
            });
        }
        let label = fields.last().copied().unwrap_or_default();
        if is_required(label) {
            metadata.insert(
                label.to_owned(),
                fields[..fields.len().saturating_sub(2)].to_vec(),
            );
        }
        if label == "KPoly" {
            kpoly_line = Some(line_number);
            continue;
        }
        if kpoly_line.is_some()
            && metadata.contains_key("Name")
            && (fields.len() != metadata["Name"].len() + 2
                || fields.last().is_some_and(|value| parse_f64(value).is_ok()))
        {
            data_start = Some(index);
            break;
        }
    }

    if kpoly_line.is_none() {
        return format_error("missing KPoly metadata row");
    }
    let data_start = data_start
        .ok_or_else(|| AcdError::Format("ACD file contains no channel-width data row".into()))?;
    for label in ["Description", "Offset", "Name", "Units"] {
        if !metadata.contains_key(label) {
            return format_error(format!("missing required metadata row(s): {label}"));
        }
    }

    let names = &metadata["Name"];
    if names.is_empty() {
        return format_error("Name metadata row contains no channels");
    }
    for label in ["Description", "Offset", "Units"] {
        if metadata[label].len() != names.len() {
            return format_error(format!(
                "{label} metadata contains {} channels; expected {}",
                metadata[label].len(),
                names.len()
            ));
        }
    }
    let blank_columns: Vec<_> = names
        .iter()
        .enumerate()
        .filter_map(|(index, name)| name.is_empty().then_some((index + 1).to_string()))
        .collect();
    if !blank_columns.is_empty() {
        return format_error(format!(
            "blank channel name(s) in column(s): {}",
            blank_columns.join(", ")
        ));
    }
    let unique_names: HashSet<_> = names.iter().collect();
    if unique_names.len() != names.len() {
        let mut duplicates: Vec<_> = names
            .iter()
            .filter(|name| names.iter().filter(|other| *other == *name).count() > 1)
            .cloned()
            .collect();
        duplicates.sort();
        duplicates.dedup();
        return format_error(format!(
            "duplicate channel name(s): {}",
            duplicates.join(", ")
        ));
    }
    let test_names: HashSet<_> = test_info.iter().map(|info| &info.name).collect();
    if test_names.len() != test_info.len() {
        return format_error("ACD test metadata names must be unique");
    }

    let mut channels: Vec<_> = names
        .iter()
        .enumerate()
        .map(|(column, name)| {
            Ok(AcdChannel {
                name: (*name).into(),
                description: metadata["Description"][column].into(),
                unit: normalize_unit(metadata["Units"][column]),
                offset: parse_offset(metadata["Offset"][column], name)?,
                column: column as u64,
                values: Vec::new(),
            })
        })
        .collect::<Result<_, AcdError>>()?;

    for (index, line) in lines[data_start..].iter().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let line_number = data_start + index + 1;
        let values = fields(line);
        if values.len() != channels.len() {
            return format_error(format!(
                "line {line_number} has {} values; expected {}",
                values.len(),
                channels.len()
            ));
        }
        for (channel, value) in channels.iter_mut().zip(values) {
            channel.values.push(parse_f64(value).map_err(|_| {
                AcdError::Format(format!(
                    "line {line_number}, channel '{}' is not numeric: {value:?}",
                    channel.name
                ))
            })?);
        }
    }
    if channels[0].values.is_empty() {
        return format_error("ACD file contains no numerical samples");
    }
    Ok(AcdMeasurement {
        encoding: encoding.into(),
        dialect: if first_content.is_some_and(|line| line.starts_with("FileInfo:")) {
            "ascii3".into()
        } else {
            "stripped".into()
        },
        channels,
        test_info,
    })
}

fn fields(line: &str) -> Vec<&str> {
    line.trim_end_matches('\r')
        .split('\t')
        .map(str::trim)
        .collect()
}

fn is_required(label: &str) -> bool {
    matches!(label, "Description" | "Offset" | "Name" | "Units")
}

fn normalize_unit(unit: &str) -> String {
    if unit.eq_ignore_ascii_case("nan") {
        String::new()
    } else {
        unit.into()
    }
}

fn parse_offset(value: &str, name: &str) -> Result<f64, AcdError> {
    let offset = parse_f64(value).map_err(|_| {
        AcdError::Format(format!(
            "offset for channel '{name}' is not numeric: {value:?}"
        ))
    })?;
    if !offset.is_finite() {
        return format_error(format!("offset for channel '{name}' must be finite"));
    }
    Ok(offset)
}

fn parse_f64(value: &str) -> Result<f64, std::num::ParseFloatError> {
    match value.to_ascii_lowercase().as_str() {
        "inf" | "+inf" | "infinity" | "+infinity" => Ok(f64::INFINITY),
        "-inf" | "-infinity" => Ok(f64::NEG_INFINITY),
        _ => value.parse(),
    }
}

fn format_error<T>(message: impl Into<String>) -> Result<T, AcdError> {
    Err(AcdError::Format(message.into()))
}

#[cfg(test)]
mod tests {
    use super::parse_bytes;

    #[test]
    fn parses_ascii3_cp1252_and_non_finite_samples() {
        let input = b"FileInfo: ASCII3\nTestInfo:\tbrakes\tbrakes\tBrakes\tinteger\t\t1\nTime\tWheel\tstring\tDescription\n0\t10\tfloat\tOffset\ntime\ttravel\tstring\tName\ns\tmm\tstring\tUnits\nNaN\tNaN\tstring\tKPoly\n0\tNaN\n1\tInf\n";
        let measurement = parse_bytes(input).unwrap();
        assert_eq!(measurement.encoding, "utf-8-sig");
        assert_eq!(measurement.dialect, "ascii3");
        assert_eq!(measurement.test_info[0].name, "brakes");
        assert_eq!(measurement.channels[1].offset, 10.0);
        assert!(measurement.channels[1].values[0].is_nan());
        assert!(measurement.channels[1].values[1].is_infinite());
    }

    #[test]
    fn parses_cp1252_and_rejects_malformed_metadata() {
        let cp1252 = b"W\xB0\tstring\tDescription\n0\tfloat\tOffset\ntime\tstring\tName\ns\tstring\tUnits\nNaN\tstring\tKPoly\n0\n";
        assert_eq!(parse_bytes(cp1252).unwrap().encoding, "cp1252");
        let error = parse_bytes(b"time\tstring\tName\n0\n").unwrap_err();
        assert!(error.to_string().contains("missing KPoly"));

        let duplicate = b"A\tB\tstring\tDescription\n0\t0\tfloat\tOffset\ntime\ttime\tstring\tName\ns\ts\tstring\tUnits\nNaN\tNaN\tstring\tKPoly\n0\t1\n";
        assert!(
            parse_bytes(duplicate)
                .unwrap_err()
                .to_string()
                .contains("duplicate channel")
        );

        let invalid_offset = b"A\tstring\tDescription\nNaN\tfloat\tOffset\ntime\tstring\tName\ns\tstring\tUnits\nNaN\tstring\tKPoly\n0\n";
        assert!(
            parse_bytes(invalid_offset)
                .unwrap_err()
                .to_string()
                .contains("offset for channel 'time' must be finite")
        );
    }
}
