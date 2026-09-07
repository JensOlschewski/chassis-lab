use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use anyhow::{Context, Result, bail};

use crate::model::{AdamsResult, Component, Entity};

#[derive(Debug)]
struct EntityHeader {
    name: String,
    source_path: Option<String>,
    object_id: Option<u64>,
    components: Vec<ComponentHeader>,
}

#[derive(Debug)]
struct ComponentHeader {
    name: String,
    adams_id: Option<usize>,
    units: Option<String>,
}

pub fn parse_file(path: &Path) -> Result<AdamsResult> {
    let file = File::open(path)
        .with_context(|| format!("failed to open Adams result file '{}'", path.display()))?;
    parse_reader(BufReader::new(file))
        .with_context(|| format!("failed to parse Adams result file '{}'", path.display()))
}

pub fn parse_reader<R: BufRead>(reader: R) -> Result<AdamsResult> {
    let mut lines = reader.lines().enumerate();
    let mut headers = parse_header(&mut lines)?;
    let values_per_sample = assign_and_validate_ids(&mut headers)?;
    let component_ids = headers
        .iter()
        .flat_map(|entity| entity.components.iter())
        .map(|component| component.adams_id.expect("IDs were assigned and validated"))
        .collect::<Vec<_>>();

    let (series, sample_count) = parse_samples(&mut lines, values_per_sample, &component_ids)?;
    let mut series = series.into_iter();

    let entities = headers
        .into_iter()
        .map(|entity| {
            let components = entity
                .components
                .into_iter()
                .map(|component| Component {
                    name: component.name,
                    adams_id: component.adams_id.expect("IDs were assigned and validated"),
                    units: component.units,
                    values: series
                        .next()
                        .expect("parser produced one series per declared component"),
                })
                .collect();

            Entity {
                name: entity.name,
                source_path: entity.source_path,
                object_id: entity.object_id,
                components,
            }
        })
        .collect();

    Ok(AdamsResult {
        entities,
        sample_count,
        values_per_sample,
    })
}

fn parse_header<I>(lines: &mut I) -> Result<Vec<EntityHeader>>
where
    I: Iterator<Item = (usize, std::io::Result<String>)>,
{
    let mut entities = Vec::new();
    let mut current_entity: Option<EntityHeader> = None;
    let mut found_first_step = false;

    for (line_index, line) in lines.by_ref() {
        let line_number = line_index + 1;
        let line = line.with_context(|| format!("failed to read line {line_number}"))?;
        let trimmed = line.trim();

        if is_step_start(trimmed) {
            found_first_step = true;
            break;
        }

        if is_entity_line(trimmed) {
            if let Some(entity) = current_entity.take() {
                entities.push(entity);
            }

            let name = extract_attribute(trimmed, "name").with_context(|| {
                format!("entity on line {line_number} has no quoted name attribute")
            })?;
            let source_path = extract_attribute(trimmed, "entity");
            let object_id = extract_attribute(trimmed, "objectId")
                .map(|value| {
                    value.parse::<u64>().with_context(|| {
                        format!(
                            "entity '{name}' on line {line_number} has invalid objectId '{value}'"
                        )
                    })
                })
                .transpose()?;

            current_entity = Some(EntityHeader {
                name,
                source_path,
                object_id,
                components: Vec::new(),
            });
            continue;
        }

        if is_component_line(trimmed) {
            let name = extract_attribute(trimmed, "name").with_context(|| {
                format!("component on line {line_number} has no quoted name attribute")
            })?;

            let adams_id = extract_attribute(trimmed, "id")
                .map(|value| {
                    value.parse::<usize>().with_context(|| {
                        format!("component '{name}' on line {line_number} has invalid id '{value}'")
                    })
                })
                .transpose()?;
            let units = extract_attribute(trimmed, "unitsValue");

            let entity = current_entity.as_mut().with_context(|| {
                format!("component '{name}' on line {line_number} appears before any entity")
            })?;
            entity.components.push(ComponentHeader {
                name,
                adams_id,
                units,
            });
        }
    }

    if let Some(entity) = current_entity {
        entities.push(entity);
    }

    if !found_first_step {
        bail!("could not find the first <Step ...> data block");
    }
    if entities.is_empty() {
        bail!("the result header does not define any entities");
    }

    for entity in &entities {
        if entity.components.is_empty() {
            bail!("entity '{}' does not define any components", entity.name);
        }
    }

    Ok(entities)
}

fn assign_and_validate_ids(headers: &mut [EntityHeader]) -> Result<usize> {
    let component_count = headers
        .iter()
        .map(|entity| entity.components.len())
        .sum::<usize>();
    let with_id = headers
        .iter()
        .flat_map(|entity| entity.components.iter())
        .filter(|component| component.adams_id.is_some())
        .count();

    if with_id == 0 {
        for (index, component) in headers
            .iter_mut()
            .flat_map(|entity| entity.components.iter_mut())
            .enumerate()
        {
            component.adams_id = Some(index + 1);
        }
        return Ok(component_count);
    }

    if with_id != component_count {
        bail!("component IDs are only present for some components; the mapping would be ambiguous");
    }

    let mut seen = std::collections::HashSet::new();
    let mut maximum_id = 0usize;
    for component in headers.iter().flat_map(|entity| entity.components.iter()) {
        let id = component.adams_id.expect("checked above");
        if id == 0 {
            bail!("component '{}' uses invalid Adams id 0", component.name);
        }
        if !seen.insert(id) {
            bail!("Adams component id {id} is declared more than once");
        }
        maximum_id = maximum_id.max(id);
    }

    Ok(maximum_id)
}

fn parse_samples<I>(
    lines: &mut I,
    values_per_sample: usize,
    component_ids: &[usize],
) -> Result<(Vec<Vec<f64>>, usize)>
where
    I: Iterator<Item = (usize, std::io::Result<String>)>,
{
    let mut series = vec![Vec::new(); component_ids.len()];
    let mut current_sample = Vec::with_capacity(values_per_sample);
    let mut in_step = true;
    let mut sample_count = 0usize;

    for (line_index, line) in lines {
        let line_number = line_index + 1;
        let line = line.with_context(|| format!("failed to read line {line_number}"))?;
        let trimmed = line.trim();

        if is_step_start(trimmed) {
            if in_step && !current_sample.is_empty() {
                bail!(
                    "encountered a new <Step> on line {line_number} before closing the previous block"
                );
            }
            in_step = true;
            continue;
        }

        if is_step_end(trimmed) {
            if in_step {
                finish_sample(
                    &mut series,
                    &mut current_sample,
                    values_per_sample,
                    component_ids,
                    sample_count + 1,
                    line_number,
                )?;
                if !series[0].is_empty() {
                    sample_count = series[0].len();
                }
            }
            in_step = false;
            continue;
        }

        if !in_step || trimmed.is_empty() {
            continue;
        }

        for token in trimmed.split_whitespace() {
            let value = parse_number(token).with_context(|| {
                format!("invalid numeric token '{token}' in data block on line {line_number}")
            })?;
            current_sample.push(value);
        }
    }

    if in_step && !current_sample.is_empty() {
        finish_sample(
            &mut series,
            &mut current_sample,
            values_per_sample,
            component_ids,
            sample_count + 1,
            0,
        )?;
        sample_count = series[0].len();
    }

    if sample_count == 0 {
        bail!("the result file does not contain any numerical data blocks");
    }

    Ok((series, sample_count))
}

fn finish_sample(
    series: &mut [Vec<f64>],
    sample: &mut Vec<f64>,
    expected_values: usize,
    component_ids: &[usize],
    sample_number: usize,
    line_number: usize,
) -> Result<()> {
    if sample.is_empty() {
        return Ok(());
    }

    if sample.len() != expected_values {
        let location = if line_number == 0 {
            "at end of file".to_owned()
        } else {
            format!("before line {line_number}")
        };
        bail!(
            "data block {sample_number} contains {} values, but the Adams ID map requires {expected_values} values ({location})",
            sample.len()
        );
    }

    for (signal, adams_id) in series.iter_mut().zip(component_ids.iter().copied()) {
        signal.push(sample[adams_id - 1]);
    }
    sample.clear();

    Ok(())
}

fn parse_number(token: &str) -> Result<f64> {
    let token = token.trim_end_matches(',');
    if token.contains('D') || token.contains('d') {
        Ok(token.replace(['D', 'd'], "E").parse()?)
    } else {
        Ok(token.parse()?)
    }
}

fn is_entity_line(line: &str) -> bool {
    line.contains("Entity name") || (line.contains("<Entity") && line.contains("name="))
}

fn is_component_line(line: &str) -> bool {
    line.contains("Component name=") || (line.contains("<Component") && line.contains("name="))
}

fn is_step_start(line: &str) -> bool {
    has_tag(line, "<Step")
}

fn is_step_end(line: &str) -> bool {
    has_tag(line, "</Step")
}

fn has_tag(line: &str, tag: &str) -> bool {
    line.match_indices(tag).any(|(index, _)| {
        line.as_bytes()
            .get(index + tag.len())
            .is_some_and(|byte| byte.is_ascii_whitespace() || *byte == b'>')
    })
}

fn extract_attribute(line: &str, attribute: &str) -> Option<String> {
    let bytes = line.as_bytes();
    let attribute_bytes = attribute.as_bytes();
    let mut cursor = 0usize;

    while cursor + attribute_bytes.len() <= bytes.len() {
        let relative = line[cursor..].find(attribute)?;
        let start = cursor + relative;
        let end = start + attribute_bytes.len();

        let valid_left_boundary =
            start == 0 || !bytes[start - 1].is_ascii_alphanumeric() && bytes[start - 1] != b'_';
        let mut value_start = end;

        while value_start < bytes.len() && bytes[value_start].is_ascii_whitespace() {
            value_start += 1;
        }

        if valid_left_boundary && bytes.get(value_start) == Some(&b'=') {
            value_start += 1;
            while value_start < bytes.len() && bytes[value_start].is_ascii_whitespace() {
                value_start += 1;
            }

            let quote = *bytes.get(value_start)?;
            if quote == b'"' || quote == b'\'' {
                value_start += 1;
                let value_end = bytes[value_start..]
                    .iter()
                    .position(|byte| *byte == quote)
                    .map(|offset| value_start + offset)?;
                return Some(line[value_start..value_end].to_owned());
            }
        }

        cursor = end;
    }

    None
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::parse_reader;

    #[test]
    fn parses_entities_components_and_steps() {
        let input = r#"
<Result>
  <Entity type="request" name="ride heights" entity="vehicle.ride_heights" objectId="42">
    <Component name="front/left" id="2" unitsValue="mm" />
    <Component name="front right" id="1" unitsValue="mm" />
  </Entity>
  <Entity name="mass_properties">
    <Component name="IXX" id="3" unitsValue="kg*mm**2" />
  </Entity>
  <Step type="input">
    1.0 2.0 3.0
  </Step>
  <Step type="input">
    4.0 5.0 6.0
  </Step>
</Result>
"#;

        let result = parse_reader(Cursor::new(input)).unwrap();

        assert_eq!(result.sample_count, 2);
        assert_eq!(result.signal_count(), 3);
        assert_eq!(result.values_per_sample, 3);
        assert_eq!(result.entities[0].name, "ride heights");
        assert_eq!(
            result.entities[0].source_path.as_deref(),
            Some("vehicle.ride_heights")
        );
        assert_eq!(result.entities[0].object_id, Some(42));
        assert_eq!(result.entities[0].components[0].name, "front/left");
        assert_eq!(result.entities[0].components[0].values, vec![2.0, 5.0]);
        assert_eq!(result.entities[0].components[0].adams_id, 2);
        assert_eq!(
            result.entities[0].components[0].units.as_deref(),
            Some("mm")
        );
        assert_eq!(result.entities[1].components[0].values, vec![3.0, 6.0]);
    }

    #[test]
    fn accepts_fortran_exponents() {
        let input = r#"
<Entity name="entity">
<Component name="value" id="1">
<Step type="input">
1.25D+02
</Step>
"#;

        let result = parse_reader(Cursor::new(input)).unwrap();
        assert_eq!(result.entities[0].components[0].values, vec![125.0]);
    }

    #[test]
    fn does_not_treat_step_map_as_a_data_block() {
        let input = r#"
<StepMap name="map_001">
  <Entity name="time">
    <Component name="TIME" unitsValue="sec" id="1" />
  </Entity>
</StepMap>
<Step type="input">
  1.5
</Step>
"#;

        let result = parse_reader(Cursor::new(input)).unwrap();
        assert_eq!(result.entities[0].components[0].values, vec![1.5]);
    }

    #[test]
    fn rejects_blocks_with_the_wrong_length() {
        let input = r#"
<Entity name="entity">
<Component name="a" id="1">
<Component name="b" id="2">
<Step type="input">
1.0
</Step>
"#;

        let error = parse_reader(Cursor::new(input)).unwrap_err();
        assert!(error.to_string().contains("contains 1 values"));
    }
    #[test]
    fn falls_back_to_header_order_when_all_ids_are_missing() {
        let input = r#"
<Entity name="entity">
<Component name="a">
<Component name="b">
<Step type="input">
10.0 20.0
</Step>
"#;

        let result = parse_reader(Cursor::new(input)).unwrap();
        assert_eq!(result.entities[0].components[0].adams_id, 1);
        assert_eq!(result.entities[0].components[1].adams_id, 2);
        assert_eq!(result.entities[0].components[1].values, vec![20.0]);
    }

    #[test]
    fn rejects_duplicate_adams_ids() {
        let input = r#"
<Entity name="entity">
<Component name="a" id="1">
<Component name="b" id="1">
<Step type="input">
10.0
</Step>
"#;

        let error = parse_reader(Cursor::new(input)).unwrap_err();
        assert!(error.to_string().contains("declared more than once"));
    }
}
