//! Bounded recipes for one independent, editable part.
use crate::{
    geom::Point3,
    model::{Id, Project},
};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    pub name: String,
    pub operations: Vec<Operation>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Operation {
    pub id: String,
    #[serde(flatten)]
    pub action: Action,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    Box { size: Point3 },
    Cylinder { radius: f64, height: f64 },
    Sphere { radius: f64 },
    Move { source: String, offset: Point3 },
    Boolean { base: String, tool: String, operation: Boolean },
    Fillet { source: String, radius: f64 },
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Boolean {
    Cut,
    Union,
    Intersect,
}
pub struct Built {
    pub project: Project,
    pub body: Id,
}
fn length(n: f64) -> Result<(), &'static str> {
    if n.is_finite() && (0.05..=2000.0).contains(&n) {
        Ok(())
    } else {
        Err("ai-error-range")
    }
}
fn take(live: &mut HashMap<String, Id>, name: &str) -> Result<Id, &'static str> {
    live.remove(name).ok_or("ai-error-reference")
}
impl Plan {
    /// Work on a new document so a failed recipe cannot change the open project.
    pub fn build(&self) -> Result<Built, &'static str> {
        if self.name.trim().is_empty() || self.name.len() > 160 || self.name.chars().any(char::is_control) || self.operations.is_empty() || self.operations.len() > 48 {
            return Err("ai-error-plan");
        }
        let mut project = Project::default();
        let part = project.new_document();
        project.components.iter_mut().find(|c| c.id == part).ok_or("ai-error-plan")?.name = self.name.clone();
        let mut live = HashMap::new();
        let mut names = HashSet::new();
        for op in &self.operations {
            if op.id.is_empty() || op.id.len() > 40 || !op.id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') || !names.insert(op.id.clone()) {
                return Err("ai-error-reference");
            }
            let body = match &op.action {
                Action::Box { size } => {
                    for n in [size.x, size.y, size.z] {
                        length(n)?;
                    }
                    project.add_box(size.x, size.y, size.z)
                }
                Action::Cylinder { radius, height } => {
                    length(*radius)?;
                    length(*height)?;
                    project.add_cylinder(*radius, *height)
                }
                Action::Sphere { radius } => {
                    length(*radius)?;
                    project.add_sphere(*radius)
                }
                Action::Move { source, offset } => {
                    if [offset.x, offset.y, offset.z].iter().any(|v| !v.is_finite() || v.abs() > 2000.0) {
                        return Err("ai-error-range");
                    }
                    let src = take(&mut live, source)?;
                    project.add_move(src, [1.0, 0.0, 0.0, offset.x, 0.0, 1.0, 0.0, offset.y, 0.0, 0.0, 1.0, offset.z])
                }
                Action::Boolean { base, tool, operation } => {
                    let a = take(&mut live, base)?;
                    let b = take(&mut live, tool)?;
                    project.add_body_boolean(
                        a,
                        b,
                        match operation {
                            Boolean::Cut => 0,
                            Boolean::Union => 1,
                            Boolean::Intersect => 2,
                        },
                    )
                }
                Action::Fillet { source, radius } => {
                    length(*radius)?;
                    let src = take(&mut live, source)?;
                    project.add_fillet(src, *radius, Vec::new())
                }
            };
            live.insert(op.id.clone(), body);
        }
        if live.len() != 1 {
            return Err("ai-error-one-body");
        }
        Ok(Built { project, body: *live.values().next().ok_or("ai-error-one-body")? })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn stock() -> Plan {
        Plan { name: "Plate".into(), operations: vec![Operation { id: "stock".into(), action: Action::Box { size: Point3::new(100.0, 60.0, 5.0) } }] }
    }
    #[test]
    fn invalid_dimensions_are_refused() {
        for value in [0.0, -1.0, f64::NAN, f64::INFINITY, 2001.0] {
            let mut p = stock();
            p.operations[0].action = Action::Cylinder { radius: value, height: 5.0 };
            assert!(p.build().is_err());
        }
    }
    #[test]
    fn duplicate_ids_and_unjoined_bodies_are_refused() {
        let mut p = stock();
        p.operations.push(p.operations[0].clone());
        assert!(p.build().is_err());
        p.operations[1].id = "other".into();
        assert!(p.build().is_err());
    }
    #[test]
    fn consumed_bodies_cannot_be_reused() {
        let mut p = stock();
        for id in ["moved", "again"] {
            p.operations.push(Operation { id: id.into(), action: Action::Move { source: "stock".into(), offset: Point3::new(10.0, 0.0, 0.0) } });
        }
        assert!(p.build().is_err());
    }
    #[test]
    fn a_recipe_retains_its_parametric_timeline() {
        let b = stock().build().unwrap();
        assert_eq!(b.project.timeline.last().unwrap().kind.body(), Some(b.body));
    }
}
