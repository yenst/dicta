use crate::catalog::{self, CatalogProject};
use dicta_core::GENERAL_PROJECT_ID;
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ProjectArgs {
    repo_path: Option<String>,
}

pub(super) fn list(args: ProjectArgs) -> Result<(String, Value), String> {
    let mut projects = catalog::load()?;
    let current = catalog::current(&projects, args.repo_path.as_deref())?;
    if let Some(current) = &current {
        if let Some(position) = projects
            .iter()
            .position(|candidate| candidate.project.id == current.project.id)
        {
            projects[position] = current.clone();
        } else {
            projects.push(current.clone());
        }
    }
    projects.sort_by(|left, right| {
        left.project
            .name
            .to_lowercase()
            .cmp(&right.project.name.to_lowercase())
            .then_with(|| left.project.id.cmp(&right.project.id))
    });

    let current_id = current.as_ref().map(|value| &value.project.id);
    let mut text = "# Dicta projects\n".to_owned();
    if projects.is_empty() {
        text.push_str("\nNo Dicta projects were found.\n");
    } else {
        for project in &projects {
            let marker = if current_id == Some(&project.project.id) {
                " (current)"
            } else {
                ""
            };
            text.push_str(&format!(
                "\n- **{}**{marker} · `{}`\n  Storage: `{}`\n",
                project.project.name,
                project.project.id,
                project.storage_path.display()
            ));
            if let Some(source) = project.project.source_path.as_deref() {
                text.push_str(&format!("  Source: `{source}`\n"));
            }
        }
    }

    Ok((
        text,
        json!({
            "current_project_id": current_id,
            "projects": projects
                .iter()
                .map(|project| project_json(project, current_id == Some(&project.project.id)))
                .collect::<Vec<_>>()
        }),
    ))
}

pub(super) fn current(args: ProjectArgs) -> Result<(String, Value), String> {
    let projects = catalog::load()?;
    let Some(project) = catalog::current(&projects, args.repo_path.as_deref())? else {
        return Ok((
            "No Dicta project could be determined for this working directory.\n".to_owned(),
            json!({ "project": null }),
        ));
    };
    let mut text = format!(
        "# Current Dicta project: {}\n\nProject ID: `{}`\nStorage: `{}`\n",
        project.project.name,
        project.project.id,
        project.storage_path.display()
    );
    if let Some(source) = project.project.source_path.as_deref() {
        text.push_str(&format!("Source: `{source}`\n"));
    }
    Ok((text, json!({ "project": project_json(&project, true) })))
}

fn project_json(project: &CatalogProject, current: bool) -> Value {
    json!({
        "id": project.project.id,
        "name": project.project.name,
        "created_at": project.project.created_at,
        "source_path": project.project.source_path,
        "storage_path": project.storage_path,
        "current": current,
        "general": project.project.id.as_str() == GENERAL_PROJECT_ID
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_json_marks_general_without_rewriting_core_metadata() {
        let project = CatalogProject {
            project: dicta_core::ProjectFile {
                id: dicta_core::ProjectId::new(GENERAL_PROJECT_ID).unwrap(),
                name: "General".to_owned(),
                created_at: std::time::UNIX_EPOCH.into(),
                source_path: Some("/tmp/general".to_owned()),
                extra: serde_json::Map::new(),
            },
            storage_path: "/tmp/general".into(),
        };
        let value = project_json(&project, false);
        assert_eq!(value["general"], true);
        assert_eq!(value["current"], false);
    }
}
