//! `dirt attach`, `dirt files`, `dirt save` and `dirt detach`: the files
//! attached to a note.
//!
//! A file is named by the start of its ID, as `dirt files` prints it, or by
//! its name if no other file of the note has the same.

use std::path::{Path, PathBuf};

use dirt_core::files::{media_type_for, safe_file_name, Attachment};
use dirt_core::store::AttachedFile;
use dirt_core::sync::engine::{file_on_device, FileError};
use dirt_core::sync::overview::size_words;
use dirt_core::{NoteId, Store};
use serde::Serialize;

use crate::commands::common::{normalize_note_identifier, resolve_note_for_edit};
use crate::context::Context;
use crate::error::CliError;

/// How much of a file's ID a list shows: as much as it shows of a note's.
const SHORT_ID: usize = 13;

async fn note_named(query: &str, store: &Store) -> Result<NoteId, CliError> {
    let query = normalize_note_identifier(query)?;
    Ok(resolve_note_for_edit(&query, store).await?.id)
}

/// The one file of a note that `query` names: by the start of its ID, or
/// by its whole name.
fn file_named<'a>(query: &str, files: &'a [AttachedFile]) -> Result<&'a AttachedFile, CliError> {
    let query = query.trim();
    if query.is_empty() {
        return Err(CliError::Files(
            "say which file: its ID or its name".to_owned(),
        ));
    }
    let lowered = query.to_ascii_lowercase();
    let by_id: Vec<&AttachedFile> = files
        .iter()
        .filter(|file| file.attachment.file_id.to_string().starts_with(&lowered))
        .collect();
    let matching = if by_id.is_empty() {
        files
            .iter()
            .filter(|file| file.attachment.name == query)
            .collect()
    } else {
        by_id
    };
    match matching.as_slice() {
        [] => Err(CliError::Files(format!(
            "the note has no file with the ID or name '{query}'. `dirt files` lists them"
        ))),
        [file] => Ok(file),
        several => Err(CliError::Files(format!(
            "'{query}' names {} files of the note: {}. Give more of the ID",
            several.len(),
            several
                .iter()
                .map(|file| file.attachment.file_id.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ))),
    }
}

/// Attaches the file at `source` to a note and prints the file's ID.
pub async fn run_attach(
    note: &str,
    source: &Path,
    name: Option<&str>,
    media_type: Option<&str>,
    context: &Context,
) -> Result<Attachment, CliError> {
    let store = &context.store;
    let note = note_named(note, store).await?;
    if !source.is_file() {
        return Err(CliError::Files(format!(
            "{} is not a file",
            source.display()
        )));
    }
    let name = name.map_or_else(
        || {
            source
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default()
        },
        str::to_owned,
    );
    // The name goes into the note as a name, never as a path.
    let name = safe_file_name(&name);
    let media_type = media_type.unwrap_or_else(|| media_type_for(&name));
    let attached = store.attach_file(&note, source, &name, media_type).await?;
    println!("{}", attached.file_id);
    context.sync_after_write().await;
    Ok(attached)
}

#[derive(Debug, Serialize)]
struct FileListItem {
    id: String,
    name: String,
    media_type: String,
    size: u64,
    has_thumbnail: bool,
    on_this_device: bool,
    /// What there is to say about where the file is, if anything.
    standing: Option<String>,
}

/// One line for each file of a note: the start of its ID, its name, its
/// size and kind, and where it is if that is worth saying.
pub async fn files_lines(note: &str, store: &Store) -> Result<Vec<String>, CliError> {
    let note = note_named(note, store).await?;
    let files = store.attached_files(&note).await?;
    if files.is_empty() {
        return Ok(vec!["The note has no files.".to_owned()]);
    }
    let widest = files
        .iter()
        .map(|file| file.attachment.name.chars().count())
        .max()
        .unwrap_or(0)
        .min(40);
    Ok(files
        .iter()
        .map(|file| {
            let attachment = &file.attachment;
            let id: String = attachment
                .file_id
                .to_string()
                .chars()
                .take(SHORT_ID)
                .collect();
            let line = format!(
                "{id}  {name:<widest$}  {size:>9}  {kind}",
                name = attachment.name,
                size = size_words(attachment.size),
                kind = attachment.media_type,
            );
            match file.standing_words() {
                Some(words) => format!("{line}  ({words})"),
                None => line,
            }
        })
        .collect())
}

pub async fn run_files(note: &str, as_json: bool, store: &Store) -> Result<(), CliError> {
    if as_json {
        let note = note_named(note, store).await?;
        let items: Vec<FileListItem> = store
            .attached_files(&note)
            .await?
            .into_iter()
            .map(|file| FileListItem {
                id: file.attachment.file_id.to_string(),
                standing: file.standing_words(),
                on_this_device: file.on_device,
                has_thumbnail: file.attachment.thumbnail.is_some(),
                name: file.attachment.name,
                media_type: file.attachment.media_type,
                size: file.attachment.size,
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&items)?);
    } else {
        for line in files_lines(note, store).await? {
            println!("{line}");
        }
    }
    Ok(())
}

/// Where a file comes to be saved: `path` if it names a file, a file of
/// the attachment's name inside it if it names a folder, and a file of
/// that name in the current folder if there is no `path`.
fn destination(path: Option<&Path>, attachment: &Attachment) -> PathBuf {
    // The name comes out of a note. It is used as a plain name only.
    let name = safe_file_name(&attachment.name);
    match path {
        Some(path) if path.is_dir() => path.join(name),
        Some(path) => path.to_path_buf(),
        None => PathBuf::from(name),
    }
}

/// Saves a copy of one of a note's files, fetching the file first if this
/// device does not hold it. Returns where the copy is.
pub async fn run_save(
    note: &str,
    file: &str,
    path: Option<&Path>,
    replace: bool,
    context: &Context,
) -> Result<PathBuf, CliError> {
    let store = &context.store;
    let note = note_named(note, store).await?;
    let files = store.attached_files(&note).await?;
    let file = file_named(file, &files)?;
    let target = destination(path, &file.attachment);
    if target.exists() && !replace {
        return Err(CliError::Files(format!(
            "{} is already there. Give another path, or pass --force to replace it",
            target.display()
        )));
    }

    let file_id = file.attachment.file_id;
    let held = match &context.session {
        Some(session) => session.fetch_file(&note, file_id).await,
        None => file_on_device(store, &note, file_id).await,
    }
    .map_err(|error| CliError::Files(why_not_fetched(&error, context.session.is_some())))?;

    std::fs::copy(&held, &target)?;
    println!("{}", target.display());
    Ok(target)
}

/// Why a file could not be had, in words that say what can be done.
fn why_not_fetched(error: &FileError, can_fetch: bool) -> String {
    match error {
        FileError::Missing if can_fetch => {
            "the file is not on this device and the server does not have it. A device \
             that holds it sends it when it next syncs"
                .to_owned()
        }
        FileError::Missing => {
            "the file is not on this device, and this device is not signed in to fetch it"
                .to_owned()
        }
        FileError::Damaged => {
            "the server's copy of the file could not be decrypted. Nothing was saved. \
             `dirt save` on a device that holds the file still works"
                .to_owned()
        }
        other => other.to_string(),
    }
}

/// Removes a file from a note and prints the file's ID.
pub async fn run_detach(note: &str, file: &str, context: &Context) -> Result<(), CliError> {
    let store = &context.store;
    let note = note_named(note, store).await?;
    let files = store.attached_files(&note).await?;
    let file_id = file_named(file, &files)?.attachment.file_id;
    store.detach_file(&note, file_id).await?;
    println!("{file_id}");
    context.sync_after_write().await;
    Ok(())
}
