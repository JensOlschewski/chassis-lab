#[derive(Debug, Clone, PartialEq)]
pub struct AdamsResult {
    pub entities: Vec<Entity>,
    pub sample_count: usize,
    pub values_per_sample: usize,
}

impl AdamsResult {
    /// Iterates over every value series and its owning entity.
    pub fn values(&self) -> impl Iterator<Item = (&Entity, &Component)> {
        self.entities.iter().flat_map(|entity| {
            entity
                .components
                .iter()
                .map(move |component| (entity, component))
        })
    }

    pub fn signal_count(&self) -> usize {
        self.values().count()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Entity {
    pub name: String,
    pub source_path: Option<String>,
    pub object_id: Option<u64>,
    pub components: Vec<Component>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Component {
    pub name: String,
    pub adams_id: usize,
    pub units: Option<String>,
    pub values: Vec<f64>,
}

#[cfg(test)]
mod tests {
    use super::{AdamsResult, Component, Entity};

    #[test]
    fn iterates_over_all_values() {
        let result = AdamsResult {
            entities: vec![Entity {
                name: "entity".into(),
                source_path: None,
                object_id: None,
                components: vec![
                    Component {
                        name: "a".into(),
                        adams_id: 1,
                        units: None,
                        values: vec![1.0],
                    },
                    Component {
                        name: "b".into(),
                        adams_id: 2,
                        units: Some("mm".into()),
                        values: vec![2.0],
                    },
                ],
            }],
            sample_count: 1,
            values_per_sample: 2,
        };

        let names = result
            .values()
            .map(|(entity, component)| format!("{}/{}", entity.name, component.name))
            .collect::<Vec<_>>();
        assert_eq!(names, ["entity/a", "entity/b"]);
    }
}
