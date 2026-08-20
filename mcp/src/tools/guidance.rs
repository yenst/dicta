use crate::{context, render, search, storage::Recording};
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct GuidanceArgs {
    repo_path: Option<String>,
    branch: Option<String>,
    query: Option<String>,
    limit: Option<u64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ListArgs {
    repo_path: Option<String>,
    branch: Option<String>,
    limit: Option<u64>,
}

pub(super) fn get(args: GuidanceArgs) -> Result<(String, Value), String> {
    let context = context::resolve(args.repo_path.as_deref(), args.branch.as_deref())?;
    let limit = super::checked_limit(args.limit, 8, 25)?;
    let mut report = context::load(&context)?;
    let query = args
        .query
        .as_deref()
        .map(str::trim)
        .filter(|query| !query.is_empty());
    let mut query_fallback = false;
    if let Some(query) = query {
        let mut scored = report
            .recordings
            .into_iter()
            .map(|recording| (search::relevance(&recording, query), recording))
            .collect::<Vec<_>>();
        if scored.iter().any(|(score, _)| *score > 0) {
            scored.retain(|(score, _)| *score > 0);
            scored.sort_by_key(|(score, _)| std::cmp::Reverse(*score));
        } else if !scored.is_empty() {
            query_fallback = true;
        }
        report.recordings = scored.into_iter().map(|(_, recording)| recording).collect();
    }
    report.recordings.truncate(limit);
    let mut text = format!(
        "# Dicta guidance: {}\n\nRepository: `{}`\nGit branch: `{}`\nPacket folder: `{}`\n",
        context.project.name,
        context.repo_root.display(),
        context.branch,
        context.branch_path.display()
    );
    if let Some(query) = query {
        text.push_str(&format!("Query: `{query}`\n"));
    }
    if report.recordings.is_empty() {
        text.push_str("\nNo matching Dicta guidance was recorded for this branch.\n");
    } else {
        if query_fallback {
            text.push_str("\nNo notes or transcripts matched the query, so the newest recordings from this branch are shown instead.\n");
        }
        text.push_str("\n## Relevant recordings\n");
        for recording in &report.recordings {
            render::append_recording_summary(&mut text, recording);
        }
    }
    render::append_warnings(&mut text, &report.warnings);
    Ok((
        text,
        result_json(&context, &report.recordings, &report.warnings),
    ))
}

pub(super) fn list(args: ListArgs) -> Result<(String, Value), String> {
    let context = context::resolve(args.repo_path.as_deref(), args.branch.as_deref())?;
    let limit = super::checked_limit(args.limit, 25, 100)?;
    let mut report = context::load(&context)?;
    report.recordings.truncate(limit);
    let mut text = format!(
        "# Dicta recordings: {} · {}\n\n",
        context.project.name, context.branch
    );
    if report.recordings.is_empty() {
        text.push_str("No repository-wide or branch recordings were found.\n");
    } else {
        for recording in &report.recordings {
            render::append_recording_summary(&mut text, recording);
        }
    }
    render::append_warnings(&mut text, &report.warnings);
    Ok((
        text,
        result_json(&context, &report.recordings, &report.warnings),
    ))
}

fn result_json(context: &context::Context, recordings: &[Recording], warnings: &[String]) -> Value {
    json!({
        "project": context.project.name,
        "repo_path": context.repo_root,
        "branch": context.branch,
        "packet_path": context.branch_path,
        "recordings": recordings.iter().map(render::recording_json).collect::<Vec<_>>(),
        "warnings": warnings
    })
}
