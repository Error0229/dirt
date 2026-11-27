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
//! preview holds in memory ([`super::served`]) the bytes of the file it
//! shows, of the files it showed most recently, within a budget, and of
//! the pictures either side of the one shown, read ahead once the script
//! says nothing moves ([`NoteFiles::read_ahead`]). It lets go of all of
//! them as it closes.

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
/// close or another note began meanwhile). Its bytes are held, as the file
/// shown, only when it is wanted; otherwise they are dropped here, and
/// nothing that was held is let go.
fn arrived(current: bool, file: Uuid, loaded: Loaded) -> Option<Showing> {
    if !current {
        return None;
    }
    Some(if loaded.kind == PreviewKind::Text {
        Showing::Text(as_text(&loaded.bytes))
    } else {
        let (media_type, bytes) = served::playable(loaded.media_type, loaded.bytes);
        match served::hold_shown(file, &loaded.name, bytes, media_type) {
            Some(address) => Showing::Media {
                kind: loaded.kind,
                address,
            },
            None => Showing::Failed("It could not be held in memory.".to_owned()),
        }
    })
}

/// Holds a picture read ahead, if it is still wanted when it comes back
/// (`wanted`: see [`wanted_ahead`]); otherwise its bytes are dropped
/// here. Its address, if it is held.
fn kept_ahead(wanted: bool, file: Uuid, loaded: Loaded) -> Option<String> {
    if !wanted {
        return None;
    }
    served::hold(file, &loaded.name, loaded.bytes, loaded.media_type)
}

/// The files either side of `at` in `all`, the next first and then the
/// one before, going round at the ends as [`NoteFiles::step`] does; never
/// `at` itself, and each once.
fn beside(all: &[Uuid], at: Uuid) -> Vec<Uuid> {
    let Some(index) = all.iter().position(|file| *file == at) else {
        return Vec::new();
    };
    let count = all.len();
    let mut found = Vec::new();
    for other in [(index + 1) % count, (index + count - 1) % count] {
        if all[other] != at && !found.contains(&all[other]) {
            found.push(all[other]);
        }
    }
    found
}

/// Whether a picture read ahead is still wanted as it comes back: the
/// preview has not closed or gone to another note since it was asked for
/// (`round`, then and now; `same_note`), and the file is the one shown
/// (stepped to while it was read) or one beside it. One the note no
/// longer has is beside nothing.
fn wanted_ahead(
    round: (u64, u64),
    same_note: bool,
    shown: Option<Uuid>,
    all: &[Uuid],
    file: Uuid,
) -> bool {
    round.0 == round.1
        && same_note
        && shown.is_some_and(|at| at == file || beside(all, at).contains(&file))
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
    /// Whether the person has unfolded the shelf of the note that is open
    /// (`folded`): `false` until they do, and again for each note.
    unfolded: Signal<bool>,
    /// How many files the note had at the first reading since it was
    /// opened, and whether a later reading found another number: what
    /// the count's live region and the shelf's coming go by (`opened_with`,
    /// `count_changed`). Kept here, not in the shelf, since the shelf's
    /// own state outlives a note closed and opened again on Android.
    opened_with: Signal<Option<usize>>,
    count_changed: Signal<bool>,
    showing: Signal<Showing>,
    /// Raised by every load the preview starts and by its closing, so that
    /// a load that comes back late shows nothing.
    loads: CopyValue<u64>,
    /// The files [`served`] holds, and their addresses, as of the last
    /// change to them: what the deck tells the script.
    held: Signal<HashMap<Uuid, String>>,
    /// Raised by every close, so that a picture read ahead that comes
    /// back after it is dropped.
    ahead: CopyValue<u64>,
    /// The pictures being read ahead.
    reading: CopyValue<HashSet<Uuid>>,
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
        unfolded: Signal::new(false),
        opened_with: Signal::new(None),
        count_changed: Signal::new(false),
        showing: Signal::new(Showing::Nothing),
        loads: CopyValue::new(0),
        held: Signal::new(HashMap::new()),
        ahead: CopyValue::new(0),
        reading: CopyValue::new(HashSet::new()),
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
            files.unfolded.set(false);
            files.opened_with.set(None);
            files.count_changed.set(false);
        }
    });

    // Of how sync goes, only what can change the files.
    let synced = use_memo(move || {
        let status = sync.read();
        (
            status.pulled,
            status.files_unsent,
            status.file_problems,
            status.syncing,
        )
    });
    // Reading the list fetches nothing, so the tiles are there at once.
    let _reader = use_resource(move || {
        let note = note();
        let _ = (files.reread)();
        let _ = synced.read();
        let mut listed = files.listed;
        async move {
            let note = note?;
            let store = files.source.call(()).store?;
            let found = store.attached_files(&note).await.unwrap_or_default();
            files.count_files(note, &found);
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

    /// The address the bytes of a file held in memory are at, while the
    /// preview is open: what the script asks for, to keep a picture
    /// decoded for a step to it.
    pub fn held_address(self, file: Uuid) -> Option<String> {
        self.held.read().get(&file).cloned()
    }

    /// Follows what [`served`] holds: it may have let go of files for its
    /// budget as it was given another.
    fn sync_held(mut self) {
        let now: HashMap<Uuid, String> = served::held_files().into_iter().collect();
        if *self.held.peek() != now {
            self.held.set(now);
        }
    }

    /// Reads the note's files again: a sync, or this device, may have
    /// changed them.
    pub fn reread(mut self) {
        *self.reread.write() += 1;
    }

    /// Whether the shelf is folded. Every note opens with it folded,
    /// whatever its count (interface design 6.5, 7.14), and only the person
    /// unfolds it: a file attached here, one a sync brings, or one removed
    /// moves nothing, so that the shelf never takes room from the text by
    /// itself. A new file's square pops into the folded line instead.
    ///
    /// It is not kept. Kept for each note it would be a view of the note
    /// that every device keeps for itself and lets go of when the note is
    /// purged, for thoughts that are opened a few times; the rule gives the
    /// same shelf each time, and the other is one press away.
    pub fn folded(self) -> bool {
        !*self.unfolded.read()
    }

    /// How many files the note had when its files were first read since it
    /// was opened; `None` before that.
    pub fn opened_with(self) -> Option<usize> {
        *self.opened_with.read()
    }

    /// Whether the number of the note's files has changed since it was
    /// opened: until it has, the count's live region says nothing.
    pub fn count_changed(self) -> bool {
        *self.count_changed.read()
    }

    /// Folds or unfolds the shelf, for as long as the note is open.
    pub fn set_folded(mut self, folded: bool) {
        self.unfolded.set(!folded);
    }

    /// Counts `found`, the files of `note`, as they are read: how many
    /// the note was opened with, and whether that has changed since, which
    /// is what the count's live region goes by.
    fn count_files(mut self, note: NoteId, found: &[AttachedFile]) {
        let same_note = matches!(self.listed.peek().as_ref(), Some((held, _)) if *held == note);
        let opened_with = *self.opened_with.peek();
        match opened_with {
            Some(first) if same_note => {
                if first != found.len() && !*self.count_changed.peek() {
                    self.count_changed.set(true);
                }
            }
            _ => self.opened_with.set(Some(found.len())),
        }
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
    /// With `ahead`, only a file the device holds is read, and nothing is
    /// fetched.
    async fn load(self, note: NoteId, file: &AttachedFile, ahead: bool) -> Result<Loaded, String> {
        let attachment = &file.attachment;
        let kind = preview_kind(&attachment.name, &attachment.media_type)
            .ok_or_else(|| "This kind of file is not shown here.".to_owned())?;
        let bytes = if ahead {
            let store = self
                .source
                .call(())
                .store
                .ok_or_else(|| "The note is not open.".to_owned())?;
            file_bytes_on_device(&store, &note, attachment.file_id)
                .await
                .map_err(|error| error.advice(false))?
        } else {
            self.bytes(note, attachment.file_id).await?
        };
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
        // Held already (shown a moment ago, or read ahead): shown at once,
        // from what is held, at the address the page may have drawn.
        let kind = preview_kind(&file.attachment.name, &file.attachment.media_type);
        if let Some(kind) = kind.filter(|kind| *kind != PreviewKind::Text) {
            if let Some(address) = served::show(id) {
                self.shown.set(Some(id));
                self.open.set(true);
                self.showing.set(Showing::Media { kind, address });
                self.sync_held();
                return;
            }
        }
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
            let loaded = self.load(note, &file, false).await;
            self.fetching.write().remove(&id);
            let current = load == self.current_load() && self.note() == Some(note);
            let showing = loaded.map(|loaded| arrived(current, id, loaded));
            self.sync_held();
            match showing {
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

    /// Reads ahead the pictures either side of the one shown, the next
    /// first, so that a step to one shows it at once (design 6.16). The
    /// script asks, once nothing in the preview moves ([`Outcome::Still`]):
    /// the track at rest, no finger on it, the preview grown.
    ///
    /// Only a picture the device holds is read, never fetched, one at a
    /// time, and not while the file shown is still being read. One that
    /// comes back after the preview has closed or gone to another note, or
    /// that is no longer beside the file shown, is dropped as it comes.
    /// Other kinds are not read ahead: what a step to a picture costs is
    /// its decoding, which the script then does at rest; a video's, a
    /// sound's or a PDF's is the web view's as it is shown, and is not
    /// kept.
    ///
    /// [`Outcome::Still`]: crate::events::Outcome::Still
    pub fn read_ahead(self) {
        let Some(at) = self.previewing_now() else {
            return;
        };
        if matches!(*self.showing.peek(), Showing::Loading | Showing::Nothing) {
            return;
        }
        let Some(note) = self.note() else { return };
        let all = self.previewable_now();
        let ids: Vec<Uuid> = all.iter().map(|file| file.attachment.file_id).collect();
        let wanted: Vec<AttachedFile> = beside(&ids, at)
            .into_iter()
            .filter_map(|id| all.iter().find(|file| file.attachment.file_id == id))
            .filter(|file| {
                let id = file.attachment.file_id;
                file.on_device
                    && preview_kind(&file.attachment.name, &file.attachment.media_type)
                        == Some(PreviewKind::Picture)
                    && !self.held.peek().contains_key(&id)
                    && !self.reading.peek().contains(&id)
            })
            .cloned()
            .collect();
        if wanted.is_empty() {
            return;
        }
        let round = *self.ahead.peek();
        let mut reading = self.reading;
        reading
            .write()
            .extend(wanted.iter().map(|file| file.attachment.file_id));
        spawn(async move {
            for file in wanted {
                let id = file.attachment.file_id;
                let read = if self.still_wanted(round, note, id) {
                    self.load(note, &file, true).await.ok()
                } else {
                    None
                };
                reading.write().remove(&id);
                let Some(loaded) = read else { continue };
                if kept_ahead(self.still_wanted(round, note, id), id, loaded).is_some() {
                    self.sync_held();
                }
            }
        });
    }

    /// See [`wanted_ahead`].
    fn still_wanted(self, round: u64, note: NoteId, file: Uuid) -> bool {
        let all: Vec<Uuid> = self
            .previewable_now()
            .iter()
            .map(|file| file.attachment.file_id)
            .collect();
        wanted_ahead(
            (round, *self.ahead.peek()),
            self.note() == Some(note),
            self.previewing_now(),
            &all,
            file,
        )
    }

    /// Closes the preview. What it showed is let go once it has gone.
    pub fn close(mut self) {
        let load = self.next_load();
        *self.ahead.write() += 1;
        // Let go of at once: what is drawn stays drawn as it shrinks away,
        // and a load or a reading ahead still running holds nothing when
        // it comes back.
        served::release_all();
        self.sync_held();
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
        let all = self.previewable_now();
        // A file the note no longer has is let go of, if it was held.
        served::release_unless(|held| all.iter().any(|file| file.attachment.file_id == held));
        self.sync_held();
        let Some(at) = self.previewing_now() else {
            return;
        };
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
        served::release(id);
        self.sync_held();
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

    fn files(count: usize) -> Vec<Uuid> {
        (0..count).map(|_| Uuid::now_v7()).collect()
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
        let [slow, quick, late, text] = files(4)[..] else {
            unreachable!()
        };

        let shown = arrived(true, quick, read("b.mp4", PreviewKind::Video));
        let Some(Showing::Media { address, .. }) = shown else {
            panic!("the file wanted is shown: {shown:?}");
        };
        assert_eq!(served::held_count(), 1);

        assert_eq!(arrived(false, slow, read("a.pdf", PreviewKind::Pdf)), None);
        assert_eq!(served::held_count(), 1);
        assert_eq!(served::answer(&address, Some("bytes=0-0")).status, 206);

        // Closed: everything is let go, and a late load holds nothing.
        served::release_all();
        assert_eq!(
            arrived(false, late, read("c.png", PreviewKind::Picture)),
            None
        );
        assert_eq!(served::held_count(), 0);

        // Text is shown as text and holds nothing.
        assert_eq!(
            arrived(true, text, read("notes.txt", PreviewKind::Text)),
            Some(Showing::Text("notes.txt".to_owned()))
        );
        assert_eq!(served::held_count(), 0);
    }

    /// A step to another file keeps what was shown, at its address, so
    /// that a step back to it is shown from memory at once; the file shown
    /// last is the one the budget keeps.
    #[test]
    fn a_file_shown_stays_held_when_the_preview_moves_on() {
        let _one_at_a_time = served::TESTS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        served::release_all();
        let [first, second] = files(2)[..] else {
            unreachable!()
        };
        let Some(Showing::Media { address, .. }) =
            arrived(true, first, read("a.jpg", PreviewKind::Picture))
        else {
            panic!("shown");
        };
        assert!(arrived(true, second, read("b.jpg", PreviewKind::Picture)).is_some());
        assert_eq!(served::held_count(), 2);
        // Back to the first: the same address, nothing read.
        assert_eq!(served::show(first), Some(address.clone()));
        assert_eq!(served::answer(&address, None).body, b"a.jpg");
        served::release_all();
        assert_eq!(served::show(first), None);
    }

    /// The files beside the one shown: the next, then the one before,
    /// going round at the ends; with two files the other once; with one,
    /// none.
    #[test]
    fn the_files_beside_go_round_at_the_ends() {
        let all = files(4);
        assert_eq!(beside(&all, all[0]), vec![all[1], all[3]]);
        assert_eq!(beside(&all, all[3]), vec![all[0], all[2]]);
        assert_eq!(beside(&all, all[1]), vec![all[2], all[0]]);
        assert_eq!(beside(&all[..2], all[0]), vec![all[1]]);
        assert_eq!(beside(&all[..1], all[0]), Vec::<Uuid>::new());
        assert_eq!(beside(&all[1..], all[0]), Vec::<Uuid>::new());
    }

    /// A picture read ahead that comes back after a close, after another
    /// note opened, after the person stepped away from it, or once its
    /// file was removed, is not held; one stepped onto while it was read
    /// is.
    #[test]
    fn a_picture_read_ahead_is_held_only_while_it_is_wanted() {
        let all = files(5);
        let shown = Some(all[2]);
        let wanted = |round, same_note, shown, all: &[Uuid], file| {
            wanted_ahead(round, same_note, shown, all, file)
        };
        // Beside the one shown, the preview as it was asked.
        assert!(wanted((1, 1), true, shown, &all, all[3]));
        assert!(wanted((1, 1), true, shown, &all, all[1]));
        // Stepped onto while it was read.
        assert!(wanted((1, 1), true, Some(all[3]), &all, all[3]));
        // Closed (and perhaps opened again) since it was asked for.
        assert!(!wanted((1, 2), true, shown, &all, all[3]));
        // Closed and not opened again.
        assert!(!wanted((1, 1), true, None, &all, all[3]));
        // Another note.
        assert!(!wanted((1, 1), false, shown, &all, all[3]));
        // Stepped on twice: no longer beside the one shown.
        assert!(!wanted((1, 1), true, Some(all[0]), &all, all[2]));
        // Removed from the note while it was read.
        let without: Vec<Uuid> = all.iter().copied().filter(|file| *file != all[3]).collect();
        assert!(!wanted((1, 1), true, shown, &without, all[3]));
    }

    /// What is read ahead is held only when it is still wanted as it
    /// comes, never in place of what is held, and never once the preview
    /// has closed: a closing lets go of everything, and what comes after
    /// it is dropped.
    #[test]
    fn a_reading_ahead_still_running_holds_nothing_after_a_close() {
        let _one_at_a_time = served::TESTS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        served::release_all();
        let [shown, next, before] = files(3)[..] else {
            unreachable!()
        };
        let Some(Showing::Media { .. }) = arrived(true, shown, read("a.jpg", PreviewKind::Picture))
        else {
            panic!("shown");
        };
        let ahead = kept_ahead(true, next, read("b.jpg", PreviewKind::Picture));
        let Some(address) = ahead else {
            panic!("a picture still wanted is held");
        };
        assert_eq!(served::held_count(), 2);
        // Read again while held: what is held stays, at its address.
        assert_eq!(
            kept_ahead(true, next, read("other.jpg", PreviewKind::Picture)),
            Some(address.clone())
        );
        assert_eq!(served::answer(&address, None).body, b"b.jpg");
        // One no longer wanted is dropped as it comes.
        assert_eq!(
            kept_ahead(false, before, read("c.jpg", PreviewKind::Picture)),
            None
        );
        assert_eq!(served::held_count(), 2);

        // Closed: all of it is let go, and a reading still running then is
        // no longer wanted (`wanted_ahead`'s round), so holds nothing.
        served::release_all();
        assert_eq!(
            kept_ahead(false, before, read("c.jpg", PreviewKind::Picture)),
            None
        );
        assert_eq!(served::held_count(), 0);
        assert_eq!(served::held_bytes(), 0);
    }
}
