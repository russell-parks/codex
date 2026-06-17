use std::path::Path;
use std::path::PathBuf;

use chrono::Datelike;

pub fn event_file_path(root: &Path, date: chrono::NaiveDate, session_id: &str) -> PathBuf {
    root.join("events")
        .join(format!("{:04}", date.year()))
        .join(format!("{:02}", date.month()))
        .join(format!("{:02}", date.day()))
        .join(format!("{session_id}.jsonl"))
}

pub fn summary_file_path(root: &Path, session_id: &str) -> PathBuf {
    root.join("runs").join(format!("{session_id}.json"))
}
