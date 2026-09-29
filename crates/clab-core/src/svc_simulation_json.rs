//! Readable SVC simulation result files.

use std::{
    fs,
    path::{Path, PathBuf},
};

use serde::Deserialize;
use thiserror::Error;

pub const FORMAT_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq)]
pub struct SignalInfo {
    pub entity: String,
    pub source_path: String,
    pub component: String,
    pub unit: String,
    pub adams_id: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Signal {
    pub info: SignalInfo,
    pub values: Vec<f64>,
}

#[derive(Debug, Error)]
pub enum SvcJsonError {
    #[error("failed to read SVC JSON '{path}': {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("invalid SVC JSON '{path}': {source}")]
    Parse {
        path: PathBuf,
        source: serde_json::Error,
    },
    #[error("invalid SVC JSON '{path}': {message}")]
    Format { path: PathBuf, message: String },
}

#[derive(Debug)]
pub struct SvcSimulationFile {
    path: PathBuf,
    source_file: String,
    sample_count: usize,
    signals: Vec<Signal>,
}

impl SvcSimulationFile {
    /// Open legacy SVC simulation JSON and validate its complete signal schema.
    ///
    /// This accepts only simulation source kind, expected schema version, finite
    /// values, and component arrays matching declared sample count.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, SvcJsonError> {
        let path = path.as_ref().to_path_buf();
        let text = fs::read_to_string(&path).map_err(|source| SvcJsonError::Read {
            path: path.clone(),
            source,
        })?;
        let raw: RawFile = serde_json::from_str(&text).map_err(|source| SvcJsonError::Parse {
            path: path.clone(),
            source,
        })?;
        if raw.schema_version != FORMAT_VERSION {
            return Err(Self::format(
                &path,
                format!(
                    "unsupported schema_version {}; expected {FORMAT_VERSION}",
                    raw.schema_version
                ),
            ));
        }
        if raw.source_kind != "simulation" {
            return Err(Self::format(
                &path,
                format!(
                    "source_kind must be 'simulation', got '{}'",
                    raw.source_kind
                ),
            ));
        }
        if raw.source_file.is_empty() {
            return Err(Self::format(&path, "source_file must not be empty"));
        }
        if raw.values_per_sample == 0 {
            return Err(Self::format(&path, "values_per_sample must be positive"));
        }
        let signal_count: usize = raw
            .entities
            .iter()
            .map(|entity| entity.components.len())
            .sum();
        if signal_count != raw.values_per_sample {
            return Err(Self::format(
                &path,
                format!(
                    "values_per_sample is {}; entities contain {signal_count} components",
                    raw.values_per_sample
                ),
            ));
        }
        if raw.entities.is_empty() {
            return Err(Self::format(&path, "entities must not be empty"));
        }

        let mut signals = Vec::new();
        for entity in raw.entities {
            if entity.components.is_empty() {
                return Err(Self::format(
                    &path,
                    format!("entity '{}' has no components", entity.name),
                ));
            }
            let _object_id = entity.object_id;
            for component in entity.components {
                if component.values.len() != raw.sample_count {
                    return Err(Self::format(
                        &path,
                        format!(
                            "signal '{}/{}' has {} values; expected {}",
                            entity.name,
                            component.name,
                            component.values.len(),
                            raw.sample_count
                        ),
                    ));
                }
                if component.values.iter().any(|value| !value.is_finite()) {
                    return Err(Self::format(
                        &path,
                        format!(
                            "signal '{}/{}' contains a non-finite value",
                            entity.name, component.name
                        ),
                    ));
                }
                signals.push(Signal {
                    info: SignalInfo {
                        entity: entity.name.clone(),
                        source_path: entity.source_path.clone().unwrap_or_default(),
                        component: component.name,
                        unit: component.units.unwrap_or_default(),
                        adams_id: component.adams_id,
                    },
                    values: component.values,
                });
            }
        }

        Ok(Self {
            path,
            source_file: raw.source_file,
            sample_count: raw.sample_count,
            signals,
        })
    }

    pub fn source_file(&self) -> &str {
        &self.source_file
    }

    pub fn sample_count(&self) -> usize {
        self.sample_count
    }

    pub fn signals(&self) -> impl Iterator<Item = &SignalInfo> {
        self.signals.iter().map(|signal| &signal.info)
    }

    pub fn matching_signals(&self, text: &str) -> Vec<&SignalInfo> {
        let query = text.to_lowercase();
        self.signals
            .iter()
            .filter_map(|signal| {
                let info = &signal.info;
                let haystack = format!(
                    "{}\t{}\t{}\t{}\t{}",
                    info.entity, info.source_path, info.component, info.unit, info.adams_id
                );
                haystack.to_lowercase().contains(&query).then_some(info)
            })
            .collect()
    }

    /// Read exactly one entity/component signal with optional unit validation.
    ///
    /// Entity names and source paths are both valid selectors. Missing or
    /// ambiguous selectors fail rather than selecting an arbitrary signal.
    pub fn read_signal(
        &self,
        entity: &str,
        component: &str,
        expected_unit: Option<&str>,
    ) -> Result<Signal, SvcJsonError> {
        let matches: Vec<_> = self
            .signals
            .iter()
            .filter(|signal| {
                signal.info.component == component
                    && (entity == signal.info.source_path || entity == signal.info.entity)
            })
            .collect();
        let signal = match matches.as_slice() {
            [signal] => signal,
            [] => {
                return Err(Self::format(
                    &self.path,
                    format!("signal '{entity}/{component}' was not found"),
                ));
            }
            matches => {
                return Err(Self::format(
                    &self.path,
                    format!(
                        "signal '{entity}/{component}' is ambiguous across {} matches",
                        matches.len()
                    ),
                ));
            }
        };
        if let Some(expected_unit) = expected_unit
            && !signal.info.unit.is_empty()
            && signal.info.unit != expected_unit
        {
            return Err(Self::format(
                &self.path,
                format!(
                    "signal '{entity}/{component}' has unit '{}', expected '{expected_unit}'",
                    signal.info.unit
                ),
            ));
        }
        Ok((*signal).clone())
    }

    fn format(path: &Path, message: impl Into<String>) -> SvcJsonError {
        SvcJsonError::Format {
            path: path.to_path_buf(),
            message: message.into(),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawFile {
    schema_version: u32,
    source_kind: String,
    source_file: String,
    sample_count: usize,
    values_per_sample: usize,
    entities: Vec<RawEntity>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawEntity {
    name: String,
    source_path: Option<String>,
    object_id: Option<u64>,
    components: Vec<RawComponent>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawComponent {
    name: String,
    adams_id: u64,
    units: Option<String>,
    values: Vec<f64>,
}
