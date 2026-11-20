//! The files of the note that is open, as both apps show them: their
//! tiles, their thumbnails, a file being fetched, and the preview
//! (interface design, sections 6.5 and 6.16).
//!
//! [`use_note_files`] gives a [`NoteFiles`], which the tiles under the
//! note and the preview over the screen share. It reads the note's files
//! from the store, again whenever a sync may have changed them; reads each
//! picture's thumbnail, fetching it once if the device does not hold it;
//! and opens the preview. What only an app can do with a file (open it in
//! another app, save a copy where the person says) the app does, with the
//! file's place on the device from [`NoteFiles::held`].
//!
//! Every byte comes from `dirt-core`, which fetches and decrypts: a file
//! the device does not hold is fetched from the server's storage and kept
//! on the device as any fetched file is ([`Session::file_bytes`]). The
//! preview holds the bytes of the one file it shows, in memory
//! ([`super::served`]), and lets go of them as it closes.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::Duration;

use dioxus::prelude::*;
use dirt_core::files::thumbnail::as_data_uri;
use dirt_core::models::NoteId;
use dirt_core::session::Session;
use dirt_core::store::AttachedFile;
use dirt_core::sync::engine::{file_bytes_on_device, file_on_device, FileError};
use dirt_core::sync::worker::SyncStatus;
use dirt_core::util::as_sentence;
use dirt_core::Store;
use uuid::Uuid;

use super::kind::{as_text, preview_kind, served_type, PreviewKind};
use super::served;
use crate::components::{use_toasts, Toast, Toasts};
use crate::icons::Icon;

/// A thumbnail larger than this is not shown. One made by Dirt is a few
/// tens of kilobytes; a note may name any file as a thumbnail.
const MOST_THUMBNAIL_BYTES: u64 = 512 * 1024;

/// How long the preview takes to close: what it showed is let go after.
const CLOSING: Duration = Duration::from_millis(450);

/// Where the app's files are: its store, and its session if it has one.
/// Asked for at the moment it is needed, since either may change.
#[derive(Clone, Default)]
pub struct FileSource {
    pub store: Option<Store>,
    pub session: Option<Session>,
}

/// What the preview shows.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Showing {
    /// Nothing: the preview is closed.
    #[default]
    Nothing,
    /// The file is being read, or fetched. A picture's thumbnail is shown
    /// meanwhile, if the device has it.
    Loading,
    /// A picture, a sound, a video or a PDF, at the address the web view
    /// asks for its bytes by.
    Media { kind: PreviewKind, address: String },
    /// A text file's text.
    Text(String),
    /// It could not be had, and why.
    Failed(String),
}

/// A file read for the preview, not yet held.
struct Loaded {
    kind: PreviewKind,
    name: String,
    media_type: &'static str,
    bytes: Vec<u8>,
}

/// What a file read for the preview shows once it has come back, or
/// `None` if it is no longer wanted (`current` is false: another file, a
/// close or another note began meanwhile). Its bytes are held, in place of
/// whatever was held, only when it is wanted; otherwise they are dropped
/// here, and nothing that was held for the file on screen is let go.
fn arrived(current: bool, loaded: Loaded) -> Option<Showing> {
    if !current {
        return None;
    }
    Some(if loaded.kind == PreviewKind::Text {
        Showing::Text(as_text(&loaded.bytes))
    } else {
        served::release_all();
        let (media_type, bytes) = served::playable(loaded.media_type, loaded.bytes);
        let address = served::hold(&loaded.name, bytes, media_type);
        Showing::Media {
            kind: loaded.kind,
            address,
        }
    })
}

/// The files of `listed` that the preview can show.
fn previewable_of(listed: Option<&(NoteId, Vec<AttachedFile>)>) -> Vec<AttachedFile> {
    listed
        .map(|(_, found)| {
            found
                .iter()
                .filter(|file| {
                    preview_kind(&file.attachment.name, &file.attachment.media_type).is_some()
                })
                .cloned()
                .collect()
        })
        .unwrap_or_default()
}

/// The open note's files, and the preview of them. Copy it freely; see the
/// module.
#[derive(Clone, Copy)]
pub struct NoteFiles {
    source: Callback<(), FileSource>,
    on_changed: Callback<()>,
    toasts: Toasts,
    /// The note, and its files as the store has them, once read.
    listed: Signal<Option<(NoteId, Vec<AttachedFile>)>>,
    reread: Signal<u64>,
    /// Each picture's thumbnail, by the picture's ID.
    thumbnails: Signal<HashMap<Uuid, String>>,
    /// Thumbnails asked of the server once already.
    asked: Signal<HashSet<Uuid>>,
    /// Files being fetched: a ring turns on their tiles.
    fetching: Signal<HashSet<Uuid>>,
    /// The file the preview shows, or showed last.
    shown: Signal<Option<Uuid>>,
    /// The preview is open.
    open: Signal<bool>,
    showing: Signal<Showing>,
    /// Raised by every load the preview starts and by its closing, so that
    /// a load that comes back late shows nothing.
    loads: CopyValue<u64>,
}

impl PartialEq for NoteFiles {
    /// One is the same as another when it is the same set of signals: a
    /// component given it again draws from it again only as they change.
    fn eq(&self, other: &Self) -> bool {
        self.listed == other.listed
    }
}

/// The files of the note `note` names.
///
/// They are read again whenever `sync` says a sync has brought or sent
/// something. `source` says where they are; `on_changed` is called when a
/// file has been removed here, for the app to read its list again.
pub fn use_note_files(
    note: ReadSignal<Option<NoteId>>,
    sync: ReadSignal<SyncStatus>,
    source: impl Fn() -> FileSource + 'static,
    on_changed: impl Fn() + 'static,
) -> NoteFiles {
    let source = use_callback(move |()| source());
    let on_changed = use_callback(move |()| on_changed());
    let toasts = use_toasts();
    let files = use_hook(|| NoteFiles {
        source,
        on_changed,
        toasts,
        listed: Signal::new(None),
        reread: Signal::new(0),
        thumbnails: Signal::new(HashMap::new()),
        asked: Signal::new(HashSet::new()),
        fetching: Signal::new(HashSet::new()),
        shown: Signal::new(None),
        open: Signal::new(false),
        showing: Signal::new(Showing::Nothing),
        loads: CopyValue::new(0),
    });

    // Another note: the preview closes, and nothing of the last note's
    // files is kept.
    use_effect(move || {
        let note = note();
        let mut files = files;
        if files.listed.peek().as_ref().map(|(held, _)| *held) != note {
            files.listed.set(None);
            files.thumbnails.write().clear();
            files.asked.write().clear();
            files.close();
            files.shown.set(None);
        }
    });

    // Reading the list fetches nothing, so the tiles are there at once.
    let _reader = use_resource(move || {
        let note = note();
        let _ = (files.reread)();
        let status = sync.read();
        let _ = (
            status.pulled,
            status.files_unsent,
            status.file_problems,
            status.syncing,
        );
        let mut listed = files.listed;
        async move {
            let note = note?;
            let store = files.source.call(()).store?;
            let found = store.attached_files(&note).await.unwrap_or_default();
            listed.set(Some((note, found)));
            files.keep_preview_in_step();
            Some(())
        }
    });

    // The thumbnails come after the tiles, each as it is had. One this
    // device holds is read from it. One it does not hold is fetched, which
    // may take long or fail: the tile shows the file's kind meanwhile, and
    // everything else about the file works without it. A fetch is tried
    // once for as long as the note is open.
    use_effect(move || {
        let Some((listed, found)) = files.listed.read().clone() else {
            return;
        };
        spawn(async move { files.read_thumbnails(note, listed, found).await });
    });

    files
}

impl NoteFiles {
    /// The note's files, in the order they were attached, once read; and
    /// which note they are of.
    pub fn listed(self) -> Option<(NoteId, Vec<AttachedFile>)> {
        self.listed.read().clone()
    }

    /// The files the preview steps through: those it can show.
    pub fn previewable(self) -> Vec<AttachedFile> {
        previewable_of(self.listed.read().as_ref())
    }

    /// The same, read without following the list: for code that has just
    /// written it, which must not be run again by its own writing.
    fn previewable_now(self) -> Vec<AttachedFile> {
        previewable_of(self.listed.peek().as_ref())
    }

    /// A picture's thumbnail, as something an `img` shows.
    pub fn thumbnail(self, file: Uuid) -> Option<String> {
        self.thumbnails.read().get(&file).cloned()
    }

    /// Whether the file is being fetched.
    pub fn is_fetching(self, file: Uuid) -> bool {
        self.fetching.read().contains(&file)
    }

    /// The file the preview shows, while it is open.
    pub fn previewing(self) -> Option<Uuid> {
        if *self.open.read() {
            *self.shown.read()
        } else {
            None
        }
    }

    /// The file the preview shows, or showed last: what it shrinks back
    /// into as it closes.
    pub fn shown_file(self) -> Option<Uuid> {
        *self.shown.read()
    }

    fn previewing_now(self) -> Option<Uuid> {
        if *self.open.peek() {
            *self.shown.peek()
        } else {
            None
        }
    }

    /// What the preview shows.
    pub fn showing(self) -> Showing {
        self.showing.read().clone()
    }

    /// Reads the note's files again: files were attached to it.
    pub fn reread(mut self) {
        *self.reread.write() += 1;
    }

    fn next_load(mut self) -> u64 {
        let mut loads = self.loads.write();
        *loads += 1;
        *loads
    }

    fn current_load(self) -> u64 {
        *self.loads.peek()
    }

    /// Reads the thumbnails of `note`'s files, while it is the note `open`
    /// names.
    ///
    /// Each step looks first: a note put away, or another opened, stops it,
    /// and what it has read for a note no longer open is dropped. The apps
    /// put the note away in the same moment as they take another store into
    /// use, so it never looks in a new store for an old note's thumbnail.
    async fn read_thumbnails(
        mut self,
        open: ReadSignal<Option<NoteId>>,
        note: NoteId,
        found: Vec<AttachedFile>,
    ) {
        let still_open = move || *open.peek() == Some(note);
        let wanted: Vec<(Uuid, Uuid, bool)> = found
            .iter()
            .filter_map(|file| {
                let thumbnail = file.attachment.thumbnail.as_ref()?;
                Some((
                    file.attachment.file_id,
                    thumbnail.file_id,
                    file.thumbnail_on_device,
                ))
            })
            .filter(|(file, _, _)| !self.thumbnails.peek().contains_key(file))
            .collect();
        let read = |thumbnail: Uuid| async move {
            if !still_open() {
                return None;
            }
            let store = self.source.call(()).store?;
            let picture = store
                .small_file(thumbnail, MOST_THUMBNAIL_BYTES)
                .await
                .ok()??;
            still_open().then(|| as_data_uri(&picture))
        };
        for (file, thumbnail, _) in wanted.iter().filter(|(_, _, here)| *here) {
            if let Some(picture) = read(*thumbnail).await {
                self.thumbnails.write().insert(*file, picture);
            }
        }
        for (file, thumbnail, _) in wanted.iter().filter(|(_, _, here)| !*here) {
            if !still_open() {
                return;
            }
            if !self.asked.write().insert(*thumbnail) {
                continue;
            }
            if self.place(note, *thumbnail, false).await.is_ok() {
                if let Some(picture) = read(*thumbnail).await {
                    self.thumbnails.write().insert(*file, picture);
                }
            }
        }
    }

    /// Where a file is on the device, fetched first if it is not here and
    /// the device is signed in. `ring` turns the ring on its tile while it
    /// is fetched.
    async fn place(mut self, note: NoteId, file: Uuid, ring: bool) -> Result<PathBuf, String> {
        let source = self.source.call(());
        if ring {
            self.fetching.write().insert(file);
        }
        let can_fetch = source.session.is_some();
        let found = match (source.session, source.store) {
            (Some(session), _) => session.fetch_file(&note, file).await,
            (None, Some(store)) => file_on_device(&store, &note, file).await,
            (None, None) => Err(FileError::NotAttached),
        };
        if ring {
            self.fetching.write().remove(&file);
        }
        found.map_err(|error| error.advice(can_fetch))
    }

    /// Where one of the open note's files is on this device, for opening it
    /// in another app or saving a copy: fetched first, with the ring on its
    /// tile, if the device does not hold it and is signed in. The path is
    /// into the store's own folder: an app copies the file out from there
    /// and never changes it. The error is said in words for the person.
    pub async fn held(self, file: &AttachedFile) -> Result<PathBuf, String> {
        let note = self
            .note()
            .ok_or_else(|| "The note is not open.".to_owned())?;
        self.place(note, file.attachment.file_id, !file.on_device)
            .await
    }

    fn note(self) -> Option<NoteId> {
        self.listed.peek().as_ref().map(|(note, _)| *note)
    }

    /// The file's bytes in memory, fetched first if need be. Nothing is
    /// written but the device's own copy of a file it had to fetch.
    async fn bytes(self, note: NoteId, file: Uuid) -> Result<Vec<u8>, String> {
        let source = self.source.call(());
        let can_fetch = source.session.is_some();
        let found = match (source.session, source.store) {
            (Some(session), _) => session.file_bytes(&note, file).await,
            (None, Some(store)) => file_bytes_on_device(&store, &note, file).await,
            (None, None) => Err(FileError::NotAttached),
        };
        found.map_err(|error| error.advice(can_fetch))
    }

    /// Reads a file for the preview. Nothing is held yet: see [`arrived`].
    async fn load(self, note: NoteId, file: &AttachedFile) -> Result<Loaded, String> {
        let attachment = &file.attachment;
        let kind = preview_kind(&attachment.name, &attachment.media_type)
            .ok_or_else(|| "This kind of file is not shown here.".to_owned())?;
        let bytes = self.bytes(note, attachment.file_id).await?;
        Ok(Loaded {
            kind,
            name: attachment.name.clone(),
            media_type: served_type(kind, &attachment.name, &attachment.media_type),
            bytes,
        })
    }

    /// Previews one of the note's files (6.16). A file on the device opens
    /// at once, showing its thumbnail while it is read. One that is not is
    /// fetched first, with the ring on its tile, and the preview opens when
    /// it has come; if it cannot be had, an error says why.
    pub fn preview(mut self, file: &AttachedFile) {
        let Some(note) = self.note() else { return };
        let id = file.attachment.file_id;
        if self.fetching.peek().contains(&id) {
            return;
        }
        let load = self.next_load();
        let open_now = file.on_device || self.previewing_now().is_some();
        if open_now {
            self.shown.set(Some(id));
            self.open.set(true);
            self.showing.set(Showing::Loading);
        } else {
            self.fetching.write().insert(id);
        }
        let file = file.clone();
        spawn(async move {
            let loaded = self.load(note, &file).await;
            self.fetching.write().remove(&id);
            let current = load == self.current_load() && self.note() == Some(note);
            match loaded.map(|loaded| arrived(current, loaded)) {
                // Another file, a close or another note came first: what
                // was read is dropped, never held.
                _ if !current => {}
                Ok(Some(showing)) => {
                    self.shown.set(Some(id));
                    self.open.set(true);
                    self.showing.set(showing);
                }
                Ok(None) => {}
                Err(why) if open_now => self.showing.set(Showing::Failed(why)),
                Err(why) => self.toasts.show_error(
                    Toast::new(format!("{} could not be fetched", file.attachment.name))
                        .icon(Icon::Alert)
                        .detail(why),
                ),
            }
            // It may have been fetched: its tile no longer says it is not
            // here.
            self.reread();
        });
    }

    /// The next file, or with `forward` false the one before, going round
    /// at the ends: → and ←, ‹ and ›, a swipe.
    pub fn step(self, forward: bool) {
        let Some(at) = self.previewing_now() else {
            return;
        };
        let all = self.previewable();
        if all.len() < 2 {
            return;
        }
        let index = all
            .iter()
            .position(|file| file.attachment.file_id == at)
            .unwrap_or(0);
        let next = if forward {
            (index + 1) % all.len()
        } else {
            (index + all.len() - 1) % all.len()
        };
        self.preview(&all[next]);
    }

    /// Closes the preview. What it showed is let go once it has gone.
    pub fn close(mut self) {
        let load = self.next_load();
        // Let go of at once: what is drawn stays drawn as it shrinks away,
        // and a load still running holds nothing when it comes back.
        served::release_all();
        if !*self.open.peek() {
            return;
        }
        self.open.set(false);
        spawn(async move {
            tokio::time::sleep(CLOSING).await;
            if load == self.current_load() && !*self.open.peek() {
                self.showing.set(Showing::Nothing);
            }
        });
    }

    /// The preview still shows a file the note has: one removed, here or
    /// on another device, gives way to the next, or the preview closes.
    /// Called by the reader of the list, which follows none of what it
    /// reads here.
    fn keep_preview_in_step(self) {
        let Some(at) = self.previewing_now() else {
            return;
        };
        let all = self.previewable_now();
        if all.iter().any(|file| file.attachment.file_id == at) {
            return;
        }
        match all.first() {
            Some(first) => self.preview(first),
            None => self.close(),
        }
    }

    /// Removes a file from the note: its tile's ✕, or the preview's. The
    /// toast says so. It cannot be undone: the device lets go of a file no
    /// note names when it next syncs.
    pub fn remove(self, file: &AttachedFile) {
        let Some(note) = self.note() else { return };
        let Some(store) = self.source.call(()).store else {
            return;
        };
        let file = file.clone();
        let id = file.attachment.file_id;
        // The preview moves on first, so that it never shows a file that
        // has gone.
        if self.previewing_now() == Some(id) {
            let others = self.previewable();
            if others.len() > 1 {
                self.step(true);
            } else {
                self.close();
            }
        }
        spawn(async move {
            match store.detach_file(&note, id).await {
                Ok(_) => self
                    .toasts
                    .show(Toast::new("File removed").icon(Icon::Trash)),
                Err(error) => self.toasts.show_error(
                    Toast::new(format!("{} was not removed", file.attachment.name))
                        .icon(Icon::Alert)
                        .detail(as_sentence(&error)),
                ),
            }
            self.on_changed.call(());
            self.reread();
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(name: &str, kind: PreviewKind) -> Loaded {
        Loaded {
            kind,
            name: name.to_owned(),
            media_type: served_type(kind, name, ""),
            bytes: name.as_bytes().to_vec(),
        }
    }

    /// Step to a file that is slow to come, then to one that is quick: the
    /// slow one, coming back late, neither lets go of the quick one on
    /// screen nor is held itself. And after a close, nothing a load still
    /// running brings back is held.
    #[test]
    fn a_load_that_comes_back_late_holds_nothing_and_frees_nothing() {
        let _one_at_a_time = served::TESTS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        served::release_all();

        let quick = arrived(true, read("b.mp4", PreviewKind::Video));
        let Some(Showing::Media { address, .. }) = quick else {
            panic!("the file wanted is shown: {quick:?}");
        };
        assert_eq!(served::held_count(), 1);

        assert_eq!(arrived(false, read("a.pdf", PreviewKind::Pdf)), None);
        assert_eq!(served::held_count(), 1);
        assert_eq!(served::answer(&address, Some("bytes=0-0")).status, 206);

        // Closed: everything is let go, and a late load holds nothing.
        served::release_all();
        assert_eq!(arrived(false, read("c.png", PreviewKind::Picture)), None);
        assert_eq!(served::held_count(), 0);

        // Text is shown as text and holds nothing.
        assert_eq!(
            arrived(true, read("notes.txt", PreviewKind::Text)),
            Some(Showing::Text("notes.txt".to_owned()))
        );
        assert_eq!(served::held_count(), 0);
    }
}
