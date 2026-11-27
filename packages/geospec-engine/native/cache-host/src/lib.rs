//! Authenticated filesystem storage for native completed evidence families
//! and for per-subject facts.
//!
//! The store's HMAC key lives in the store root, so the envelope binds entries
//! to one installation and detects corruption and divergence; it is not a
//! boundary against a local writer who can read the directory.
//!
//! Evidence families are content-addressed, write-once per action and
//! published under the store lock with every file and directory synced.
//!
//! Facts are published without the lock and without a sync. A fact is one
//! record file, `facts/<aa>/<action>.fact`: the canonical action record, one
//! newline, then the payload. It is written to a temporary name unique to the
//! handle and made visible by a no-replace hard link. Capacity is accounted by
//! reservation: a handle reserves entries and bytes in the ledger in chunks,
//! under the lock and synced, before it publishes anything they cover, and
//! returns the unused remainder at flush or drop.
//!
//! What a crash, a kill or a concurrent writer can leave, and what `load` does:
//!
//! | State on disk | How it arises | `load` |
//! | --- | --- | --- |
//! | no record | never published, evicted, lost in a power loss | miss |
//! | temporary only | killed before the link | miss; collected after an hour |
//! | record and its temporary (two links) | killed between link and unlink | hit; the temporary is counted twice until collected after an hour |
//! | complete record | normal | verified, served |
//! | empty, short, zero-filled or stale record | power loss before writeback | length, SHA-256 or HMAC fails: miss; the next publication replaces it |
//! | two valid, different publications | nondeterministic producer | the second poisons; the marker wins over any record |
//! | ledger older than the entries | power loss after an unsynced return | over-states |
//! | ledger missing or malformed | damage | recounted |
//!
//! A published fact survives any process exit at once and a power loss once
//! the operating system has written it back, typically within tens of
//! seconds. The ledger may over-state by one chunk per dead writer until the
//! next recount; after a recount it may under-state by at most the unspent
//! chunk of each live writer until that writer's next checkpoint. So the
//! store holds at most the caps plus one maximum chunk per concurrent writer.
//! `managed-v3` and older directories are left in place and never read or
//! collected.

use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, HashSet},
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    rc::Rc,
    time::{Duration, Instant, SystemTime},
};

#[cfg(unix)]
use std::os::fd::AsRawFd;

use geospec_engine_native_core::{
    cache::{EvidenceAddress, OverlapEvidenceCache},
    canonicalize, hex, sha256_hex,
};
use hmac::{Hmac, KeyInit, Mac};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const ENVELOPE_SCHEMA: &str = "geospec-authenticated-evidence-v1";
const SECRET_BYTES: usize = 32;
const MAX_PAYLOAD_BYTES: usize = 16 * 1024 * 1024;
const MAX_ACTION_BYTES: usize = 64 * 1024;
const MAX_FACT_RECORD_BYTES: usize = MAX_ACTION_BYTES + 1 + MAX_PAYLOAD_BYTES;
const OVERLAP_FAMILY: &str = "native-b48-overlap-evidence-v1";
const EXACT_FAMILY: &str = "native-exact-step-clusters-v1";
const FACTS_FAMILY: &str = "geospec-facts-v1";
const FAMILIES: [(&str, &str, Class); 3] = [
    (
        OVERLAP_FAMILY,
        "geospec-overlap-evidence-json-v1",
        Class::Evidence,
    ),
    (
        EXACT_FAMILY,
        "geospec-exact-clusters-json-v1",
        Class::Evidence,
    ),
    (FACTS_FAMILY, "geospec-facts-json-v1", Class::Facts),
];
/// v4 keeps evidence in `actions/` and `content/` as v3 did and each fact as
/// one record in `facts/`, all sharded by the first two digest characters.
/// v3 and older directories are left untouched and miss.
const MANAGED_DIRECTORY: &str = "managed-v4";
const LEDGER_SCHEMA: &str = "geospec-cache-ledger-v2";
const MAX_EVIDENCE_ACTIONS: usize = 256;
/// One small action per (subject, scope, fact): tens of thousands of subjects.
const MAX_FACT_ACTIONS: usize = 65_536;
const MAX_STORE_BYTES: u64 = 512 * 1024 * 1024;
const LOW_WATER_BYTES: u64 = 384 * 1024 * 1024;
const COLLECTION_SLICE: usize = 16;
/// An evidence publication waits this long for a writer; a read never takes
/// the lock.
const PUBLISH_WAIT: Duration = Duration::from_secs(1);
/// A fact reservation checkpoint, a repair or a poisoning waits this long.
const CHECKPOINT_WAIT: Duration = Duration::from_secs(5);
/// `flush` waits this long to return a reservation; `Drop` does not wait.
const RETURN_WAIT: Duration = Duration::from_millis(50);
/// A temporary younger than this may belong to a live writer.
const TEMPORARY_AGE: Duration = Duration::from_secs(60 * 60);
const FIRST_CHUNK_ENTRIES: u64 = 32;
const FIRST_CHUNK_BYTES: u64 = 1024 * 1024;
const POISON_MARKER: &[u8] = b"divergent action\n";
type HmacSha256 = Hmac<Sha256>;

/// Families share one store but not one action cap: per-fact actions are
/// small and numerous, completed evidence is large and few.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Class {
    Evidence,
    Facts,
}

fn class_of(family: &str, codec: &str) -> Option<Class> {
    FAMILIES
        .iter()
        .find(|(known, known_codec, _)| *known == family && *known_codec == codec)
        .map(|(_, _, class)| *class)
}

#[derive(Clone, Copy)]
struct Limits {
    actions: usize,
    fact_actions: usize,
    bytes: u64,
    low_water: u64,
    slice: usize,
    temporary_age: Duration,
}

impl Limits {
    /// Eviction for a class over its count stops at this many actions.
    fn low_water_actions(&self, class: Class) -> usize {
        match class {
            Class::Evidence => self.actions,
            Class::Facts => self.fact_actions - self.fact_actions / 8,
        }
    }

    /// The largest fact reservation: a 64th of the room eviction makes, so
    /// many writers checkpointing near the cap do not evict more often than
    /// once per that room (128 entries and 2 MiB by default).
    fn chunk_entries(&self) -> u64 {
        ((self.fact_actions - self.low_water_actions(Class::Facts)) as u64 / 64).max(1)
    }

    fn chunk_bytes(&self) -> u64 {
        ((self.bytes - self.low_water) / 64).max(1)
    }

    /// A hard bound on directory entries an inventory reads before refusing.
    fn inventory_bound(&self) -> usize {
        2 * (self.actions + self.fact_actions) + 4 * self.slice + 2 + 256
    }
}

const DEFAULT_LIMITS: Limits = Limits {
    actions: MAX_EVIDENCE_ACTIONS,
    fact_actions: MAX_FACT_ACTIONS,
    bytes: MAX_STORE_BYTES,
    low_water: LOW_WATER_BYTES,
    slice: COLLECTION_SLICE,
    temporary_age: TEMPORARY_AGE,
};

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct Diagnostics {
    reads: u64,
    hits: u64,
    misses: u64,
    writes: u64,
    io_failures: u64,
    authentication_failures: u64,
    rejected_writes: u64,
    sealed: bool,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Envelope {
    schema: String,
    authority_id: String,
    key_id: String,
    action_sha256: String,
    family: String,
    codec: String,
    producer_profile_sha256: String,
    content_sha256: String,
    content_byte_length: u64,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct ActionRecord {
    envelope: Envelope,
    hmac_sha256: String,
}

struct IndexedAction {
    path: PathBuf,
    record: ActionRecord,
    class: Class,
    modified: SystemTime,
    bytes: u64,
    poisoned: bool,
}

struct IndexedContent {
    path: PathBuf,
    digest: Option<String>,
    bytes: u64,
}

struct Inventory {
    actions: Vec<IndexedAction>,
    content: Vec<IndexedContent>,
    /// Every temporary in any shard, with its modification time.
    temporaries: Vec<(PathBuf, SystemTime)>,
    /// Action files and fact records that fail to parse or authenticate,
    /// evidence actions that lack content, and fact records beside a poison
    /// marker. A read of any of them is already a miss.
    invalid_actions: Vec<PathBuf>,
    /// Every file in the action and fact shards, poison markers and
    /// temporaries included.
    #[cfg(test)]
    entries: usize,
    bytes: u64,
}

impl Inventory {
    fn ledger(&self) -> Ledger {
        let count = |class| {
            self.actions
                .iter()
                .filter(|action| action.class == class)
                .count() as u64
        };
        Ledger {
            epoch: 0,
            evidence: count(Class::Evidence),
            facts: count(Class::Facts),
            bytes: self.bytes,
            pending: false,
        }
    }
}

/// Action counts and stored bytes, maintained under the lock so publication
/// never scans the store. Fact counts include the reservations of live
/// handles. `epoch` changes with every recount, which voids every
/// outstanding reservation. `pending` marks an evidence publication or a
/// poisoning in progress; a missing, malformed or pending ledger makes the
/// next locked operation recount from a full inventory.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct Ledger {
    epoch: u64,
    evidence: u64,
    facts: u64,
    bytes: u64,
    pending: bool,
}

impl Ledger {
    fn with(mut self, class: Class, actions: i64, bytes: i64) -> Self {
        let field = match class {
            Class::Evidence => &mut self.evidence,
            Class::Facts => &mut self.facts,
        };
        *field = field.saturating_add_signed(actions);
        self.bytes = self.bytes.saturating_add_signed(bytes);
        self
    }

    fn encode(&self) -> Vec<u8> {
        format!(
            "{LEDGER_SCHEMA} {} {} {} {} {}\n",
            self.epoch,
            self.evidence,
            self.facts,
            self.bytes,
            if self.pending { "pending" } else { "clean" }
        )
        .into_bytes()
    }

    fn decode(bytes: &[u8]) -> Option<Self> {
        let text = std::str::from_utf8(bytes).ok()?.strip_suffix('\n')?;
        let mut fields = text.split(' ');
        if fields.next()? != LEDGER_SCHEMA {
            return None;
        }
        let mut number = || fields.next()?.parse::<u64>().ok();
        let (epoch, evidence, facts, total) = (number()?, number()?, number()?, number()?);
        let pending = match fields.next()? {
            "pending" => true,
            "clean" => false,
            _ => return None,
        };
        let ledger = Self {
            epoch,
            evidence,
            facts,
            bytes: total,
            pending,
        };
        (fields.next().is_none() && ledger.encode() == bytes).then_some(ledger)
    }
}

/// The part of the ledger's fact count and bytes this handle reserved and
/// has not spent, and what it spent from its current chunk.
#[derive(Clone, Copy, Debug, Default)]
struct Reservation {
    /// The ledger epoch the current chunk was added under; `None` before the
    /// first checkpoint and after the remainder was returned.
    epoch: Option<u64>,
    entries: u64,
    bytes: u64,
    used_entries: u64,
    used_bytes: u64,
    /// The nominal size of the last chunk, doubled at each checkpoint.
    chunk_entries: u64,
    chunk_bytes: u64,
}

/// What a no-replace link found at a fact's final name.
enum Existing {
    Missing,
    Same,
    /// A complete record that authenticates under this key and differs.
    Divergent,
    /// A complete record that fails integrity: replaceable.
    Damaged,
}

enum Placed {
    Stored,
    Repeat,
}

#[derive(Debug)]
enum FactInvalid {
    Malformed,
    Authentication,
}

/// Capacity one publication needs, the ledger that showed it does not fit,
/// and the action eviction must keep.
struct Need<'a> {
    seen: Ledger,
    evidence: u64,
    facts: u64,
    bytes: u64,
    keep: Option<&'a str>,
}

/// One explicit, process-local adapter over an authenticated filesystem root.
pub struct AuthenticatedOverlapCache {
    root: PathBuf,
    lock_path: PathBuf,
    limits: Limits,
    b48_generation: Cell<u64>,
    secret: [u8; SECRET_BYTES],
    authority_id: String,
    key_id: String,
    diagnostics: RefCell<Diagnostics>,
    sealed: Cell<bool>,
    /// Set when a publication met a full file system; later ones are skipped.
    disk_full: Cell<bool>,
    /// Set when the file system refused a hard link; later facts are skipped.
    links_unsupported: Cell<bool>,
    flush_result: RefCell<Option<Vec<u8>>>,
    /// 128 random bits naming this handle's temporaries.
    handle_id: String,
    temporaries: Cell<u64>,
    reservation: Cell<Reservation>,
    #[cfg(test)]
    checkpoints: Cell<u64>,
}

impl AuthenticatedOverlapCache {
    /// Opens a cache outside the project tree and creates its installation key
    /// with exclusive atomic first publication when needed.
    pub fn open(root: impl AsRef<Path>, project_root: impl AsRef<Path>) -> io::Result<Self> {
        Self::open_with_limits(root, project_root, DEFAULT_LIMITS)
    }

    fn open_with_limits(
        root: impl AsRef<Path>,
        project_root: impl AsRef<Path>,
        limits: Limits,
    ) -> io::Result<Self> {
        #[cfg(not(unix))]
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "GeoSpec optional persistent cache requires a supported OS file lock.",
        ));
        #[cfg(unix)]
        if limits.actions < 2
            || limits.fact_actions < 8
            || limits.slice == 0
            || limits.low_water >= limits.bytes
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid cache limits",
            ));
        }
        // Overlap is decided before anything is created, so a refused root
        // never leaves a directory inside the project.
        let project = fs::canonicalize(project_root)?;
        let overlaps = |root: &Path| root.starts_with(&project) || project.starts_with(root);
        if overlaps(&prospective_canonical(root.as_ref())?) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "GeoSpec cache root must not overlap the project tree.",
            ));
        }
        fs::create_dir_all(root.as_ref())?;
        let root = fs::canonicalize(root)?;
        if overlaps(&root) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "GeoSpec cache root must not overlap the project tree.",
            ));
        }
        let secret = load_or_create_secret(&root)?;
        // Older binaries use root/actions, root/content, managed-v2 or
        // managed-v3. They remain untouched; this generation safely misses
        // them.
        let managed = root.join(MANAGED_DIRECTORY);
        ensure_plain_directory(&managed)?;
        let root = fs::canonicalize(&managed)?;
        if !root.starts_with(&fs::canonicalize(
            managed.parent().expect("managed root parent"),
        )?) || root.starts_with(&project)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "cache directory escaped authority root",
            ));
        }
        ensure_plain_directory(&root.join("content"))?;
        ensure_plain_directory(&root.join("actions"))?;
        ensure_plain_directory(&root.join("facts"))?;
        let lock_path = root.join("cache.lock");
        let mut options = OpenOptions::new();
        options.write(true).create(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
        }
        options.open(&lock_path)?;
        let mut handle = [0_u8; 16];
        fill_random(&mut handle)?;
        let cache = Self {
            authority_id: domain_id(b"geospec-cache-authority-v1\0", &secret),
            key_id: domain_id(b"geospec-cache-key-v1\0", &secret),
            root,
            lock_path,
            limits,
            b48_generation: Cell::new(0),
            secret,
            diagnostics: RefCell::new(Diagnostics::default()),
            sealed: Cell::new(false),
            disk_full: Cell::new(false),
            links_unsupported: Cell::new(false),
            flush_result: RefCell::new(None),
            handle_id: hex(&handle),
            temporaries: Cell::new(0),
            reservation: Cell::new(Reservation::default()),
            #[cfg(test)]
            checkpoints: Cell::new(0),
        };
        // Recover from a missing or pending ledger: a bounded number of
        // blob-before-action crash leftovers and temporaries older than the
        // age limit are reclaimed and the ledger is recounted. A clean ledger
        // costs one small read. A failed inventory never deletes ambiguous
        // data.
        match cache.with_lock_wait(PUBLISH_WAIT, || {
            cache.b48_generation.set(cache.load_or_create_generation()?);
            cache.ledger().map(|_| ())
        }) {
            Ok(()) => {}
            // Another process holds the lock: open without recovery when the
            // generation already exists, since reads take no lock.
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                cache.b48_generation.set(cache.read_generation()?);
            }
            Err(error) => return Err(error),
        }
        Ok(cache)
    }

    /// Seals publication, returns the unspent fact reservation and returns
    /// the same canonical diagnostics on every later call.
    pub fn flush(&self) -> Vec<u8> {
        if let Some(result) = self.flush_result.borrow().as_ref() {
            return result.clone();
        }
        self.sealed.set(true);
        self.return_reservation(RETURN_WAIT);
        self.diagnostics.borrow_mut().sealed = true;
        let bytes = canonicalize(
            &serde_json::to_vec(&*self.diagnostics.borrow()).expect("diagnostics serialize"),
        )
        .expect("diagnostics canonicalize");
        *self.flush_result.borrow_mut() = Some(bytes.clone());
        bytes
    }

    /// Removes authenticated B48 actions, and actions and fact records that
    /// fail to authenticate, in this managed generation. Content shared with
    /// other actions and poison markers remain; a crash before completion may
    /// leave B48 actions, but never acknowledges the clear.
    pub fn clear(&self) -> bool {
        if self.sealed.get() {
            return false;
        }
        let result = self.with_lock(|| {
            let inventory = self.inventory()?;
            let next = self.read_generation()?.checked_add(1).ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "cache generation exhausted")
            })?;
            self.write_generation(next)?;
            self.write_ledger(
                Ledger {
                    pending: true,
                    ..inventory.ledger()
                },
                true,
            )?;
            for action in &inventory.actions {
                if action.record.envelope.family == OVERLAP_FAMILY && !action.poisoned {
                    remove_present(&action.path)?;
                    sync_parent(&action.path)?;
                }
            }
            // An action that fails to authenticate belongs to no family and
            // never hits; clearing removes it rather than refusing.
            for path in &inventory.invalid_actions {
                remove_present(path)?;
                sync_parent(path)?;
            }
            self.b48_generation.set(next);
            self.recount()
        });
        if result.is_err() {
            self.diagnostics.borrow_mut().io_failures += 1;
        }
        result.is_ok()
    }
}

impl Drop for AuthenticatedOverlapCache {
    /// Returns the unspent reservation with one attempt at the lock: this runs
    /// in a binding's finalizer, which must not wait. A skipped return only
    /// over-states.
    fn drop(&mut self) {
        self.return_reservation(Duration::ZERO);
    }
}

/// A view of a store that serves only the `geospec-facts-v1` family. Every
/// other family misses and is never written, exactly as with no store, so a
/// host that opens a store for facts does not start persisting completed
/// overlap or exact-cluster evidence across runs.
pub struct FactsOnlyCache(Rc<AuthenticatedOverlapCache>);

impl FactsOnlyCache {
    pub fn new(cache: Rc<AuthenticatedOverlapCache>) -> Self {
        Self(cache)
    }
}

impl OverlapEvidenceCache for FactsOnlyCache {
    fn load(&self, address: &EvidenceAddress) -> Option<Vec<u8>> {
        (address.family == FACTS_FAMILY)
            .then(|| self.0.load(address))
            .flatten()
    }

    fn publish(&self, address: &EvidenceAddress, payload: &[u8]) {
        if address.family == FACTS_FAMILY {
            self.0.publish(address, payload);
        }
    }
}

impl OverlapEvidenceCache for AuthenticatedOverlapCache {
    /// Lock-free: entries are immutable files published by atomic link, and
    /// the poison marker is checked before and after.
    fn load(&self, address: &EvidenceAddress) -> Option<Vec<u8>> {
        self.diagnostics.borrow_mut().reads += 1;
        let result = if valid_address(address)
            && class_of(address.family, address.codec) == Some(Class::Facts)
        {
            self.load_fact(address)
        } else {
            self.load_inner(address)
        };
        let mut diagnostics = self.diagnostics.borrow_mut();
        if result.is_some() {
            diagnostics.hits += 1;
        } else {
            diagnostics.misses += 1;
        }
        result
    }

    fn publish(&self, address: &EvidenceAddress, payload: &[u8]) {
        if self.sealed.get() || self.disk_full.get() || payload.len() > MAX_PAYLOAD_BYTES {
            self.diagnostics.borrow_mut().rejected_writes += 1;
            return;
        }
        let fact =
            valid_address(address) && class_of(address.family, address.codec) == Some(Class::Facts);
        if fact && self.links_unsupported.get() {
            self.diagnostics.borrow_mut().rejected_writes += 1;
            return;
        }
        let result = if fact {
            self.publish_fact(address, payload)
        } else {
            self.with_lock_wait(PUBLISH_WAIT, || self.publish_inner(address, payload))
        };
        match result {
            Ok(()) => self.diagnostics.borrow_mut().writes += 1,
            // A full file system stops publication from this handle, so a
            // full disk costs one failed publication, not one per fact.
            Err(error) if is_disk_full(&error) => {
                self.disk_full.set(true);
                self.diagnostics.borrow_mut().io_failures += 1;
            }
            // So does a file system without hard links (a copied store).
            Err(error) if error.kind() == io::ErrorKind::Unsupported => {
                self.links_unsupported.set(true);
                self.diagnostics.borrow_mut().io_failures += 1;
            }
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock
                        | io::ErrorKind::StorageFull
                        | io::ErrorKind::Interrupted
                        | io::ErrorKind::InvalidInput
                ) =>
            {
                self.diagnostics.borrow_mut().rejected_writes += 1;
            }
            Err(_) => self.diagnostics.borrow_mut().io_failures += 1,
        }
    }
}

impl AuthenticatedOverlapCache {
    fn generation_path(&self) -> PathBuf {
        self.root.join("b48-generation")
    }

    fn ledger_path(&self) -> PathBuf {
        self.root.join("ledger")
    }

    fn action_path(&self, digest: &str, extension: &str) -> PathBuf {
        self.root
            .join("actions")
            .join(&digest[..2])
            .join(format!("{digest}.{extension}"))
    }

    fn content_path(&self, digest: &str) -> PathBuf {
        self.root
            .join("content")
            .join(&digest[..2])
            .join(format!("{digest}.json"))
    }

    fn fact_path(&self, digest: &str, extension: &str) -> PathBuf {
        self.root
            .join("facts")
            .join(&digest[..2])
            .join(format!("{digest}.{extension}"))
    }

    fn load_or_create_generation(&self) -> io::Result<u64> {
        let abandoned = self.root.join("b48-generation.tmp");
        if abandoned.exists() {
            let metadata = fs::symlink_metadata(&abandoned)?;
            if !metadata.file_type().is_file() || metadata.len() > 21 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "unknown cache generation temporary",
                ));
            }
            fs::remove_file(&abandoned)?;
            sync_file(&File::open(&self.root)?)?;
        }
        let path = self.generation_path();
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut file) => {
                file.write_all(b"0\n")?;
                sync_file(&file)?;
                sync_file(&File::open(&self.root)?)?;
                Ok(0)
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => self.read_generation(),
            Err(error) => Err(error),
        }
    }

    fn read_generation(&self) -> io::Result<u64> {
        let path = self.generation_path();
        if !fs::symlink_metadata(&path)?.file_type().is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid cache generation",
            ));
        }
        let bytes = read_bounded(&path, 21)?;
        let value = std::str::from_utf8(&bytes).map_err(io::Error::other)?;
        let number = value
            .strip_suffix('\n')
            .unwrap_or_default()
            .parse::<u64>()
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid cache generation"))?;
        if bytes != format!("{number}\n").as_bytes() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "noncanonical cache generation",
            ));
        }
        Ok(number)
    }

    fn write_generation(&self, generation: u64) -> io::Result<()> {
        let path = self.generation_path();
        let temporary = self.root.join("b48-generation.tmp");
        let result = (|| {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)?;
            file.write_all(format!("{generation}\n").as_bytes())?;
            sync_file(&file)?;
            fs::rename(&temporary, &path)?;
            sync_file(&File::open(&self.root)?)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result
    }

    /// The ledger as it is on disk, or `None` when it is missing, malformed
    /// or pending. Lock held.
    fn read_ledger(&self) -> io::Result<Option<Ledger>> {
        match read_bounded(&self.ledger_path(), 128) {
            Ok(bytes) => Ok(Ledger::decode(&bytes).filter(|ledger| !ledger.pending)),
            // Missing, or not a small plain file (oversized): recount.
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::NotFound | io::ErrorKind::InvalidData
                ) =>
            {
                Ok(None)
            }
            Err(error) => Err(error),
        }
    }

    /// The ledger, recounted from a full inventory when it is missing,
    /// malformed or left pending by an interrupted publication. Lock held.
    fn ledger(&self) -> io::Result<Ledger> {
        if let Some(ledger) = self.read_ledger()? {
            return Ok(ledger);
        }
        self.collect_orphans(self.limits.slice)?;
        self.recount()
    }

    /// Rewrites the ledger from a full inventory under a fresh epoch, which
    /// voids every outstanding reservation. Lock held.
    fn recount(&self) -> io::Result<Ledger> {
        let ledger = Ledger {
            epoch: fresh_epoch()?,
            ..self.inventory()?.ledger()
        };
        self.write_ledger(ledger, false)?;
        Ok(ledger)
    }

    /// Replaces the ledger by rename. A pending ledger and a reservation are
    /// durable (file and root directory synced) before any entry they
    /// announce is written, so a crash can lose a later ledger but never
    /// those. Lock held.
    fn write_ledger(&self, ledger: Ledger, durable: bool) -> io::Result<()> {
        let durable = durable || ledger.pending;
        let temporary = self.root.join("ledger.tmp");
        let mut options = OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NOFOLLOW);
        }
        let mut file = options.open(&temporary)?;
        file.write_all(&ledger.encode())?;
        if durable {
            sync_file(&file)?;
        }
        drop(file);
        fs::rename(&temporary, self.ledger_path())?;
        if durable {
            sync_file(&File::open(&self.root)?)?;
        }
        Ok(())
    }

    fn with_lock<T>(&self, operation: impl FnOnce() -> io::Result<T>) -> io::Result<T> {
        self.with_lock_wait(Duration::ZERO, operation)
    }

    fn with_lock_wait<T>(
        &self,
        maximum_wait: Duration,
        operation: impl FnOnce() -> io::Result<T>,
    ) -> io::Result<T> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            let deadline = Instant::now() + maximum_wait;
            let lock = loop {
                let file = OpenOptions::new()
                    .read(true)
                    .custom_flags(libc::O_NOFOLLOW)
                    .open(&self.lock_path)?;
                if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
                    break file;
                }
                let error = io::Error::last_os_error();
                if error.kind() != io::ErrorKind::WouldBlock || Instant::now() >= deadline {
                    return Err(error);
                }
                std::thread::sleep(Duration::from_millis(1));
            };
            // Closing this descriptor releases its process-independent flock,
            // including when the operation returns an error or unwinds.
            let result = operation();
            drop(lock);
            result
        }
        #[cfg(not(unix))]
        {
            let _ = maximum_wait;
            let _ = operation;
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "OS cache lock unavailable",
            ))
        }
    }

    fn read_action(&self, path: &Path) -> io::Result<(ActionRecord, Class)> {
        if !fs::symlink_metadata(path)?.file_type().is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "cache action is not a file",
            ));
        }
        let bytes = read_bounded(path, MAX_ACTION_BYTES)?;
        if canonicalize(&bytes).map_err(io::Error::other)? != bytes {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "noncanonical cache action",
            ));
        }
        let record: ActionRecord = serde_json::from_slice(&bytes).map_err(io::Error::other)?;
        let envelope = &record.envelope;
        let name = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or_default();
        let class = class_of(&envelope.family, &envelope.codec);
        if !valid_digest(name)
            || envelope.action_sha256 != name
            || envelope.schema != ENVELOPE_SCHEMA
            || envelope.authority_id != self.authority_id
            || envelope.key_id != self.key_id
            || !valid_digest(&envelope.content_sha256)
            || envelope.content_byte_length > MAX_PAYLOAD_BYTES as u64
            || class != Some(Class::Evidence)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "unknown or invalid cache action",
            ));
        }
        if !self.authenticates(envelope, &record.hmac_sha256) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "cache action authentication failed",
            ));
        }
        Ok((record, Class::Evidence))
    }

    fn authenticates(&self, envelope: &Envelope, supplied: &str) -> bool {
        let Ok(bytes) = serde_json::to_vec(envelope) else {
            return false;
        };
        let Ok(envelope_bytes) = canonicalize(&bytes) else {
            return false;
        };
        let Ok(mut mac) = HmacSha256::new_from_slice(&self.secret) else {
            return false;
        };
        mac.update(&envelope_bytes);
        decode_hex_32(supplied).is_some_and(|supplied| mac.verify_slice(&supplied).is_ok())
    }

    /// The integrity check of a fact record named `action`: one canonical
    /// action record, a newline, then a payload whose length and SHA-256 the
    /// envelope names, the whole authenticated under this installation's key.
    /// It returns where the payload starts. Identity (family, codec,
    /// producer profile, installation) is the caller's to compare.
    fn verify_fact(
        &self,
        bytes: &[u8],
        action: &str,
    ) -> Result<(ActionRecord, usize), FactInvalid> {
        let split = bytes
            .iter()
            .position(|byte| *byte == b'\n')
            .ok_or(FactInvalid::Malformed)?;
        let header = &bytes[..split];
        let payload = &bytes[split + 1..];
        if header.len() > MAX_ACTION_BYTES
            || canonicalize(header).map_err(|_| FactInvalid::Malformed)? != header
        {
            return Err(FactInvalid::Malformed);
        }
        let record: ActionRecord =
            serde_json::from_slice(header).map_err(|_| FactInvalid::Malformed)?;
        if !self.authenticates(&record.envelope, &record.hmac_sha256) {
            return Err(FactInvalid::Authentication);
        }
        let envelope = &record.envelope;
        if envelope.action_sha256 != action
            || envelope.content_byte_length != payload.len() as u64
            || sha256_hex(payload) != envelope.content_sha256
        {
            return Err(FactInvalid::Malformed);
        }
        Ok((record, split + 1))
    }

    /// Whether an authenticated envelope belongs to this installation and to
    /// the fact class; identity beyond what `verify_fact` checks.
    fn fact_identity(&self, envelope: &Envelope) -> bool {
        envelope.schema == ENVELOPE_SCHEMA
            && envelope.authority_id == self.authority_id
            && envelope.key_id == self.key_id
            && class_of(&envelope.family, &envelope.codec) == Some(Class::Facts)
    }

    /// The files of every two-character shard of `directory` that carry a
    /// digest stem in their own shard. Anything else is skipped and left in
    /// place: no read serves it, and one foreign entry must not stop
    /// recovery and eviction for the whole store. Skipped entries still
    /// count toward the inventory bound. A file that disappears while it is
    /// listed (a writer's temporary) is skipped.
    fn shard_files(&self, directory: &str) -> io::Result<Vec<(PathBuf, fs::Metadata, String)>> {
        let mut files = Vec::new();
        let mut seen = 0_usize;
        let mut visit = || {
            seen += 1;
            if seen > self.limits.inventory_bound() {
                Err(io::Error::new(
                    io::ErrorKind::StorageFull,
                    "cache inventory exceeds bound",
                ))
            } else {
                Ok(())
            }
        };
        for shard in fs::read_dir(self.root.join(directory))? {
            let shard = shard?;
            visit()?;
            let shard_path = shard.path();
            let shard_name = shard.file_name();
            let shard_name = shard_name.to_str().unwrap_or_default();
            if shard_name.len() != 2
                || !shard_name
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
                || !fs::symlink_metadata(&shard_path)?.file_type().is_dir()
            {
                continue;
            }
            for entry in fs::read_dir(&shard_path)? {
                let entry = entry?;
                visit()?;
                let path = entry.path();
                let metadata = match fs::symlink_metadata(&path) {
                    Ok(metadata) => metadata,
                    Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                    Err(error) => return Err(error),
                };
                let digest = path
                    .file_stem()
                    .and_then(|stem| stem.to_str())
                    .unwrap_or_default()
                    .to_owned();
                if !metadata.file_type().is_file()
                    || !valid_digest(&digest)
                    || &digest[..2] != shard_name
                {
                    continue;
                }
                files.push((path, metadata, digest));
            }
        }
        Ok(files)
    }

    /// A hard-bounded inventory, used for recovery, eviction and clear only.
    /// Unknown entries are skipped. An action file or fact record that fails
    /// to parse or authenticate, an evidence action whose content is missing
    /// and a fact record beside a poison marker are collected as orphans: a
    /// read of each is already a miss. An operating-system error refuses.
    fn inventory(&self) -> io::Result<Inventory> {
        #[cfg(test)]
        test_support::INVENTORIES.with(|count| count.set(count.get() + 1));
        let mut actions = Vec::new();
        let mut temporaries = Vec::new();
        let mut invalid_actions = Vec::new();
        let mut poison = HashSet::new();
        let mut bytes = 0_u64;
        let files = self.shard_files("actions")?;
        #[cfg(test)]
        let mut entries = files.len();
        for (path, metadata, digest) in files {
            match path.extension().and_then(|extension| extension.to_str()) {
                Some("poison") => {
                    bytes = bytes.saturating_add(metadata.len());
                    poison.insert(digest);
                }
                Some("json") => {
                    bytes = bytes.saturating_add(metadata.len());
                    match self.read_action(&path) {
                        Ok((record, class)) => actions.push(IndexedAction {
                            record,
                            class,
                            modified: metadata.modified()?,
                            path,
                            bytes: metadata.len(),
                            poisoned: false,
                        }),
                        Err(error) if error.raw_os_error().is_some() => return Err(error),
                        Err(_) => invalid_actions.push(path),
                    }
                }
                Some(extension) if extension.starts_with("tmp-") => {
                    bytes = bytes.saturating_add(metadata.len());
                    temporaries.push((path, metadata.modified()?));
                }
                _ => {}
            }
        }
        for action in &mut actions {
            action.poisoned = poison.contains(&action.record.envelope.action_sha256);
        }
        let files = self.shard_files("facts")?;
        #[cfg(test)]
        {
            entries += files.len();
        }
        let mut fact_poison = HashSet::new();
        let mut facts = Vec::new();
        for (path, metadata, digest) in files {
            match path.extension().and_then(|extension| extension.to_str()) {
                Some("poison") => {
                    bytes = bytes.saturating_add(metadata.len());
                    fact_poison.insert(digest);
                }
                Some("fact") => {
                    bytes = bytes.saturating_add(metadata.len());
                    let record = match read_bounded(&path, MAX_FACT_RECORD_BYTES) {
                        Ok(record) => record,
                        Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                        Err(error) if error.raw_os_error().is_some() => return Err(error),
                        Err(_) => {
                            invalid_actions.push(path);
                            continue;
                        }
                    };
                    match self.verify_fact(&record, &digest) {
                        Ok((record, _)) if self.fact_identity(&record.envelope) => {
                            facts.push(IndexedAction {
                                record,
                                class: Class::Facts,
                                modified: metadata.modified()?,
                                path,
                                bytes: metadata.len(),
                                poisoned: false,
                            })
                        }
                        _ => invalid_actions.push(path),
                    }
                }
                Some(extension) if extension.starts_with("tmp-") => {
                    bytes = bytes.saturating_add(metadata.len());
                    temporaries.push((path, metadata.modified()?));
                }
                _ => {}
            }
        }
        // A record linked beside a poison marker is never served.
        let (poisoned, facts): (Vec<_>, Vec<_>) = facts
            .into_iter()
            .partition(|fact| fact_poison.contains(&fact.record.envelope.action_sha256));
        invalid_actions.extend(poisoned.into_iter().map(|fact| fact.path));
        let mut content = Vec::new();
        for (path, metadata, stem) in self.shard_files("content")? {
            let extension = path
                .extension()
                .and_then(|value| value.to_str())
                .unwrap_or_default();
            if extension.starts_with("tmp-") {
                bytes = bytes.saturating_add(metadata.len());
                temporaries.push((path, metadata.modified()?));
                continue;
            }
            if extension != "json" {
                continue;
            }
            bytes = bytes.saturating_add(metadata.len());
            // An oversized blob can back no valid action: an orphan.
            let digest = (metadata.len() <= MAX_PAYLOAD_BYTES as u64).then_some(stem);
            content.push(IndexedContent {
                path,
                digest,
                bytes: metadata.len(),
            });
        }
        let present: HashSet<String> = content
            .iter()
            .filter_map(|blob| blob.digest.clone())
            .collect();
        let (actions, lacking): (Vec<_>, Vec<_>) = actions
            .into_iter()
            .partition(|action| present.contains(action.record.envelope.content_sha256.as_str()));
        invalid_actions.extend(lacking.into_iter().map(|action| action.path));
        let mut actions = actions;
        actions.extend(facts);
        Ok(Inventory {
            actions,
            content,
            temporaries,
            invalid_actions,
            #[cfg(test)]
            entries,
            bytes,
        })
    }

    /// Whether a temporary is old enough that no live writer can own it.
    /// A modification time in the future is young.
    fn aged(&self, modified: SystemTime) -> bool {
        SystemTime::now()
            .duration_since(modified)
            .is_ok_and(|age| age >= self.limits.temporary_age)
    }

    /// Removes at most `quota` aged temporaries, invalid actions and blobs
    /// no kept action references, and syncs each touched shard once. A young
    /// temporary may belong to a live writer and is never removed. Lock held.
    fn remove_orphans(
        &self,
        inventory: &Inventory,
        referenced: &HashSet<&str>,
        quota: usize,
        shards: &mut HashSet<PathBuf>,
    ) -> io::Result<()> {
        let aged = inventory
            .temporaries
            .iter()
            .filter(|(_, modified)| self.aged(*modified))
            .map(|(path, _)| path);
        let blobs = inventory
            .content
            .iter()
            .filter(|blob| {
                blob.digest
                    .as_deref()
                    .is_none_or(|digest| !referenced.contains(digest))
            })
            .map(|blob| &blob.path);
        for path in aged
            .chain(&inventory.invalid_actions)
            .chain(blobs)
            .take(quota)
        {
            remove_present(path)?;
            shards.insert(
                path.parent()
                    .expect("cache entry has a parent")
                    .to_path_buf(),
            );
        }
        Ok(())
    }

    /// Removes at most `quota` orphans. Lock held.
    fn collect_orphans(&self, quota: usize) -> io::Result<()> {
        let inventory = self.inventory()?;
        let referenced: HashSet<_> = inventory
            .actions
            .iter()
            .filter(|action| action.class == Class::Evidence)
            .map(|action| action.record.envelope.content_sha256.as_str())
            .collect();
        let mut shards = HashSet::new();
        self.remove_orphans(&inventory, &referenced, quota, &mut shards)?;
        sync_directories(&shards)
    }

    /// One store-wide eviction pass: one inventory, then the oldest
    /// unpoisoned actions of any class are deleted until every dimension
    /// that `need` overflows is at its low-water mark, orphans are collected,
    /// each touched shard is synced once and the ledger is recounted under a
    /// fresh epoch. Lock held.
    fn evict(&self, need: &Need) -> io::Result<Ledger> {
        #[cfg(test)]
        test_support::EVICTIONS.with(|count| count.set(count.get() + 1));
        let mut inventory = self.inventory()?;
        inventory.actions.sort_by_key(|action| action.modified);
        let mut current = inventory.ledger();
        let limits = &self.limits;
        // A dimension the caller's ledger overflowed is evicted to its low
        // water even when the inventory is just under the cap: writers that
        // publish without the lock may fill it before the recount.
        let target = |count: u64, needed: u64, cap: u64, low: u64| {
            (count.saturating_add(needed) > cap).then(|| low.min(cap.saturating_sub(needed)))
        };
        let seen = &need.seen;
        let evidence_target = target(
            current.evidence.max(seen.evidence),
            need.evidence,
            limits.actions as u64,
            limits.low_water_actions(Class::Evidence) as u64,
        );
        let facts_target = target(
            current.facts.max(seen.facts),
            need.facts,
            limits.fact_actions as u64,
            limits.low_water_actions(Class::Facts) as u64,
        );
        let bytes_target = target(
            current.bytes.max(seen.bytes),
            need.bytes,
            limits.bytes,
            limits.low_water,
        );
        let mut references: HashMap<&str, u64> = HashMap::new();
        for action in &inventory.actions {
            if action.class == Class::Evidence {
                *references
                    .entry(action.record.envelope.content_sha256.as_str())
                    .or_default() += 1;
            }
        }
        let blob_bytes: HashMap<&str, u64> = inventory
            .content
            .iter()
            .filter_map(|blob| Some((blob.digest.as_deref()?, blob.bytes)))
            .collect();
        let over = |current: &Ledger, class: Class| {
            let count = match class {
                Class::Evidence => evidence_target.is_some_and(|low| current.evidence > low),
                Class::Facts => facts_target.is_some_and(|low| current.facts > low),
            };
            count || bytes_target.is_some_and(|low| current.bytes > low)
        };
        let mut shards = HashSet::new();
        for action in &inventory.actions {
            if !over(&current, Class::Evidence) && !over(&current, Class::Facts) {
                break;
            }
            if action.poisoned
                || need.keep == Some(action.record.envelope.action_sha256.as_str())
                || !over(&current, action.class)
            {
                continue;
            }
            remove_present(&action.path)?;
            shards.insert(
                action
                    .path
                    .parent()
                    .expect("cache entry has a parent")
                    .to_path_buf(),
            );
            let mut freed = action.bytes;
            if action.class == Class::Evidence {
                let digest = action.record.envelope.content_sha256.as_str();
                let left = references.entry(digest).or_default();
                *left = left.saturating_sub(1);
                if *left == 0 {
                    freed += blob_bytes.get(digest).copied().unwrap_or_default();
                }
            }
            current = current.with(action.class, -1, -(freed as i64));
        }
        let referenced: HashSet<&str> = references
            .iter()
            .filter(|(_, count)| **count > 0)
            .map(|(digest, _)| *digest)
            .collect();
        self.remove_orphans(&inventory, &referenced, usize::MAX, &mut shards)?;
        // One sync per touched shard rather than per removal bounds the
        // work at the cap; removals are durable before the recount.
        sync_directories(&shards)?;
        self.recount()
    }

    /// Makes room for `need` from the ledger alone unless eviction is
    /// needed. Lock held.
    fn ensure_capacity(&self, need: &Need) -> io::Result<Ledger> {
        let fits = |ledger: &Ledger| {
            ledger.evidence.saturating_add(need.evidence) <= self.limits.actions as u64
                && ledger.facts.saturating_add(need.facts) <= self.limits.fact_actions as u64
                && ledger.bytes.saturating_add(need.bytes) <= self.limits.bytes
        };
        let ledger = self.ledger()?;
        if fits(&ledger) {
            return Ok(ledger);
        }
        let ledger = self.evict(&Need {
            seen: ledger,
            ..*need
        })?;
        if fits(&ledger) {
            Ok(ledger)
        } else {
            Err(io::Error::new(
                io::ErrorKind::StorageFull,
                "optional cache capacity exhausted",
            ))
        }
    }

    fn load_inner(&self, address: &EvidenceAddress) -> Option<Vec<u8>> {
        if !valid_address(address) {
            return None;
        }
        let action_path = self.action_path(&address.action_sha256, "json");
        let poison_path = self.action_path(&address.action_sha256, "poison");
        if poison_path.exists() {
            return None;
        }
        let action = read_bounded(&action_path, MAX_ACTION_BYTES).ok()?;
        if canonicalize(&action).ok()? != action {
            return None;
        }
        let record: ActionRecord = serde_json::from_slice(&action).ok()?;
        if !self.authenticates(&record.envelope, &record.hmac_sha256) {
            self.diagnostics.borrow_mut().authentication_failures += 1;
            return None;
        }
        let envelope = record.envelope;
        if envelope.schema != ENVELOPE_SCHEMA
            || envelope.authority_id != self.authority_id
            || envelope.key_id != self.key_id
            || envelope.action_sha256 != address.action_sha256
            || envelope.family != address.family
            || envelope.codec != address.codec
            || envelope.producer_profile_sha256 != address.producer_profile_sha256
            || !valid_digest(&envelope.content_sha256)
            || envelope.content_byte_length > MAX_PAYLOAD_BYTES as u64
        {
            return None;
        }
        let payload = read_bounded(
            &self.content_path(&envelope.content_sha256),
            envelope.content_byte_length as usize,
        )
        .ok()?;
        if payload.len() as u64 != envelope.content_byte_length
            || sha256_hex(&payload) != envelope.content_sha256
            || poison_path.exists()
        {
            return None;
        }
        Some(payload)
    }

    /// One bounded read of the record, its integrity check, its identity
    /// against the address and the poison marker before and after.
    fn load_fact(&self, address: &EvidenceAddress) -> Option<Vec<u8>> {
        let path = self.fact_path(&address.action_sha256, "fact");
        let poison = path.with_extension("poison");
        if poison.exists() {
            return None;
        }
        let mut bytes = read_bounded(&path, MAX_FACT_RECORD_BYTES).ok()?;
        let (record, start) = match self.verify_fact(&bytes, &address.action_sha256) {
            Ok(verified) => verified,
            Err(FactInvalid::Authentication) => {
                self.diagnostics.borrow_mut().authentication_failures += 1;
                return None;
            }
            Err(FactInvalid::Malformed) => return None,
        };
        let envelope = &record.envelope;
        if !self.fact_identity(envelope)
            || envelope.family != address.family
            || envelope.codec != address.codec
            || envelope.producer_profile_sha256 != address.producer_profile_sha256
            || poison.exists()
        {
            return None;
        }
        Some(bytes.split_off(start))
    }

    /// Poisons a divergent action and removes it. Lock held.
    fn poison(&self, action: &Path, class: Class) -> io::Result<()> {
        let ledger = self.ledger()?;
        // Pending (and durable) before the marker exists, so a crash between
        // the marker and the clean ledger is recounted, never under-counted.
        self.write_ledger(
            Ledger {
                pending: true,
                ..ledger.with(class, 0, POISON_MARKER.len() as i64)
            },
            true,
        )?;
        let poison = action.with_extension("poison");
        publish_immutable(&poison, POISON_MARKER)?;
        let marker = POISON_MARKER.len() as i64;
        let ledger = match fs::symlink_metadata(action) {
            Ok(metadata) => {
                fs::remove_file(action)?;
                sync_parent(action)?;
                ledger.with(class, -1, marker - metadata.len() as i64)
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => ledger.with(class, 0, marker),
            Err(error) => return Err(error),
        };
        self.write_ledger(ledger, false)
    }

    fn publish_inner(&self, address: &EvidenceAddress, payload: &[u8]) -> io::Result<()> {
        if !valid_address(address) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid evidence address",
            ));
        }
        let class = class_of(address.family, address.codec).expect("valid address family");
        if address.family == OVERLAP_FAMILY && self.read_generation()? != self.b48_generation.get()
        {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "stale B48 cache generation",
            ));
        }
        let (record, bytes) = self.action_record(address, payload)?;
        let content_sha256 = record.envelope.content_sha256;
        let content = self.content_path(&content_sha256);
        let action = self.action_path(&address.action_sha256, "json");
        if action.with_extension("poison").exists() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "poisoned evidence action",
            ));
        }
        if action.exists() {
            // The existing record's own class is the one the ledger counted.
            let (_, existing_class) = self.read_action(&action)?;
            let existing = read_bounded(&action, MAX_ACTION_BYTES)?;
            let existing_content = read_bounded(&content, MAX_PAYLOAD_BYTES);
            if existing == bytes && existing_content.is_ok_and(|stored| stored == payload) {
                return Ok(());
            }
            self.poison(&action, existing_class)?;
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "divergent evidence action",
            ));
        }
        // Capacity is reserved for the content too: ensure_capacity may
        // collect an orphan blob with this digest, so whether the content is
        // new is decided only after it ran.
        let reserved = bytes.len() as u64 + payload.len() as u64;
        let ledger = self.ensure_capacity(&Need {
            // Set by ensure_capacity from the ledger it reads.
            seen: Ledger::default(),
            evidence: 1,
            facts: 0,
            bytes: reserved,
            keep: Some(&address.action_sha256),
        })?;
        let new_content = !content.exists();
        let added = bytes.len() as u64 + if new_content { payload.len() as u64 } else { 0 };
        let next = ledger.with(class, 1, added as i64);
        self.write_ledger(
            Ledger {
                pending: true,
                ..next
            },
            true,
        )?;
        // A content blob is named by the digest of its bytes, so an existing
        // blob with other bytes is corrupt and serves no valid action: it is
        // collected and written once more. Only a conflicting action file
        // poisons the action; a content conflict never does.
        let replaced = Cell::new(false);
        let written = (|| {
            ensure_plain_directory(content.parent().expect("content shard"))
                .map_err(|error| (false, error))?;
            ensure_plain_directory(action.parent().expect("action shard"))
                .map_err(|error| (false, error))?;
            match publish_immutable(&content, payload) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                    fs::remove_file(&content).map_err(|error| (false, error))?;
                    sync_parent(&content).map_err(|error| (false, error))?;
                    replaced.set(true);
                    publish_immutable(&content, payload).map_err(|error| (false, error))?;
                }
                Err(error) => return Err((false, error)),
            }
            publish_immutable(&action, &bytes).map_err(|error| (true, error))
        })();
        // A replaced blob is counted again in full, so the ledger only
        // over-states.
        let next = if replaced.get() {
            next.with(class, 0, payload.len() as i64)
        } else {
            next
        };
        match written {
            Ok(()) => self.write_ledger(next, false),
            Err((true, error)) if error.kind() == io::ErrorKind::AlreadyExists => {
                self.poison(&action, class)?;
                Err(error)
            }
            Err((_, error)) => {
                // `next` counts this entry whether or not it landed, so it
                // only over-states; marking it clean keeps a failed write
                // (a full disk) from forcing a full recount on every later
                // publication. Orphans are collected at the next eviction.
                let _ = self.write_ledger(next, false);
                Err(error)
            }
        }
    }

    /// The authenticated action record for `payload` and its canonical bytes.
    fn action_record(
        &self,
        address: &EvidenceAddress,
        payload: &[u8],
    ) -> io::Result<(ActionRecord, Vec<u8>)> {
        let envelope = Envelope {
            schema: ENVELOPE_SCHEMA.into(),
            authority_id: self.authority_id.clone(),
            key_id: self.key_id.clone(),
            action_sha256: address.action_sha256.clone(),
            family: address.family.into(),
            codec: address.codec.into(),
            producer_profile_sha256: address.producer_profile_sha256.clone(),
            content_sha256: sha256_hex(payload),
            content_byte_length: payload.len() as u64,
        };
        let envelope_bytes =
            canonicalize(&serde_json::to_vec(&envelope).map_err(io::Error::other)?)
                .map_err(io::Error::other)?;
        let mut mac = HmacSha256::new_from_slice(&self.secret)
            .map_err(|_| io::Error::other("invalid HMAC key"))?;
        mac.update(&envelope_bytes);
        let record = ActionRecord {
            envelope,
            hmac_sha256: hex(&mac.finalize().into_bytes()),
        };
        let bytes = canonicalize(&serde_json::to_vec(&record).map_err(io::Error::other)?)
            .map_err(io::Error::other)?;
        Ok((record, bytes))
    }

    /// Publishes one fact without the lock and without a sync: a record
    /// written to this handle's temporary and linked into place, taking one
    /// entry and its bytes from the handle's reservation. A publication that
    /// stores nothing gives its unit back.
    fn publish_fact(&self, address: &EvidenceAddress, payload: &[u8]) -> io::Result<()> {
        let (_, mut record) = self.action_record(address, payload)?;
        record.push(b'\n');
        record.extend_from_slice(payload);
        let path = self.fact_path(&address.action_sha256, "fact");
        if path.with_extension("poison").exists() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "poisoned fact action",
            ));
        }
        let length = record.len() as u64;
        self.reserve(length)?;
        match self.place_fact(&path, &record) {
            Ok(Placed::Stored) => Ok(()),
            Ok(Placed::Repeat) => {
                self.unreserve(length);
                Ok(())
            }
            Err(error) => {
                self.unreserve(length);
                Err(error)
            }
        }
    }

    fn place_fact(&self, path: &Path, record: &[u8]) -> io::Result<Placed> {
        let counter = self.temporaries.get();
        self.temporaries.set(counter + 1);
        let digest = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .expect("fact path has a digest stem");
        let temporary = path.with_file_name(format!("{digest}.tmp-{}-{counter}", self.handle_id));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NOFOLLOW);
        }
        // A collision with an existing file refuses; that file is another
        // writer's and is never removed here.
        let mut file = match options.open(&temporary) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                ensure_plain_directory(path.parent().expect("fact shard"))?;
                options.open(&temporary)?
            }
            other => other?,
        };
        let result = (|| {
            test_point("created", &temporary)?;
            file.write_all(record)?;
            drop(file);
            test_point("written", &temporary)?;
            self.link_fact(&temporary, path, record)
        })();
        // After a link the temporary is a second name of the record; after a
        // repair it no longer exists. Neither removal is needed for
        // correctness: a leftover is collected once it is an hour old.
        let _ = fs::remove_file(&temporary);
        if result.is_ok() {
            test_point("unlinked", &temporary)?;
        }
        result
    }

    /// Links the written temporary to the record's name without replacing
    /// anything, and settles a name that is already taken.
    fn link_fact(&self, temporary: &Path, path: &Path, record: &[u8]) -> io::Result<Placed> {
        for _ in 0..2 {
            match fs::hard_link(temporary, path) {
                Ok(()) => {
                    test_point("linked", path)?;
                    return Ok(Placed::Stored);
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(link_error(error)),
            }
            match self.existing_fact(path, record)? {
                // Evicted or poisoned between the link and the read: link
                // once more.
                Existing::Missing => continue,
                Existing::Same => return Ok(Placed::Repeat),
                Existing::Divergent | Existing::Damaged => {
                    return self.with_lock_wait(CHECKPOINT_WAIT, || {
                        self.settle_fact(temporary, path, record)
                    });
                }
            }
        }
        Err(io::Error::other("fact record vanished twice while linking"))
    }

    /// Settles a taken name under the lock, with the same classification as
    /// the lock-free path: a record that still fails integrity is replaced,
    /// one that authenticates and differs is poisoned. Lock held.
    fn settle_fact(&self, temporary: &Path, path: &Path, record: &[u8]) -> io::Result<Placed> {
        for _ in 0..2 {
            if path.with_extension("poison").exists() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "poisoned fact action",
                ));
            }
            match self.existing_fact(path, record)? {
                Existing::Missing => match fs::hard_link(temporary, path) {
                    Ok(()) => return Ok(Placed::Stored),
                    // A lock-free writer linked in between: classify again.
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                    Err(error) => return Err(link_error(error)),
                },
                Existing::Same => return Ok(Placed::Repeat),
                Existing::Divergent => {
                    self.poison_fact(path)?;
                    return Err(io::Error::new(
                        io::ErrorKind::AlreadyExists,
                        "divergent fact action",
                    ));
                }
                Existing::Damaged => {
                    fs::rename(temporary, path)?;
                    return Ok(Placed::Stored);
                }
            }
        }
        Err(io::Error::other("fact record changed twice while settling"))
    }

    /// Reads the record at a taken name. Only a complete read decides: an
    /// operating-system error, an oversized file or one that is not a plain
    /// file aborts the publication without replacing or poisoning anything.
    fn existing_fact(&self, path: &Path, record: &[u8]) -> io::Result<Existing> {
        test_point("existing", path)?;
        let bytes = match read_bounded(path, MAX_FACT_RECORD_BYTES) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Existing::Missing),
            Err(error) => return Err(error),
        };
        let digest = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or_default();
        Ok(if bytes == record {
            Existing::Same
        } else if self.verify_fact(&bytes, digest).is_ok() {
            Existing::Divergent
        } else {
            Existing::Damaged
        })
    }

    /// Poisons a divergent fact: the marker is durable before the record is
    /// removed, and the ledger counts the marker before it exists. Lock held.
    fn poison_fact(&self, path: &Path) -> io::Result<()> {
        let marker = path.with_extension("poison");
        let ledger = self.ledger()?;
        self.write_ledger(
            ledger.with(Class::Facts, 0, POISON_MARKER.len() as i64),
            true,
        )?;
        publish_immutable(&marker, POISON_MARKER)?;
        // The removed record stays counted until the next recount: the
        // ledger over-states.
        remove_present(path)
    }

    /// Takes one entry and `bytes` from the reservation, checkpointing first
    /// when it is short.
    fn reserve(&self, bytes: u64) -> io::Result<()> {
        let current = self.reservation.get();
        if current.epoch.is_none() || current.entries < 1 || current.bytes < bytes {
            self.with_lock_wait(CHECKPOINT_WAIT, || self.checkpoint(bytes))?;
        }
        let mut reservation = self.reservation.get();
        reservation.entries -= 1;
        reservation.bytes -= bytes;
        reservation.used_entries += 1;
        reservation.used_bytes += bytes;
        self.reservation.set(reservation);
        Ok(())
    }

    /// Gives back a unit `reserve` took for a publication that stored nothing.
    fn unreserve(&self, bytes: u64) {
        let mut reservation = self.reservation.get();
        reservation.entries += 1;
        reservation.bytes += bytes;
        reservation.used_entries = reservation.used_entries.saturating_sub(1);
        reservation.used_bytes = reservation.used_bytes.saturating_sub(bytes);
        self.reservation.set(reservation);
    }

    /// Settles the current chunk and reserves the next, at least `bytes`
    /// large, evicting first when it would not fit. The new reservation is
    /// durable before any record it covers is written. Lock held.
    fn checkpoint(&self, bytes: u64) -> io::Result<()> {
        #[cfg(test)]
        self.checkpoints.set(self.checkpoints.get() + 1);
        let current = self.reservation.get();
        let mut ledger = self.ledger()?;
        ledger = match current.epoch {
            // The unspent remainder is still in the ledger: return it.
            Some(epoch) if epoch == ledger.epoch => ledger.with(
                Class::Facts,
                -(current.entries as i64),
                -(current.bytes as i64),
            ),
            // A recount voided the chunk; what was published from it after
            // the recount is not counted. Re-adding all of it over-states.
            Some(_) => ledger.with(
                Class::Facts,
                current.used_entries as i64,
                current.used_bytes as i64,
            ),
            None => ledger,
        };
        let (chunk_entries, chunk_bytes) = if current.chunk_entries == 0 {
            (
                FIRST_CHUNK_ENTRIES.min(self.limits.chunk_entries()),
                FIRST_CHUNK_BYTES.min(self.limits.chunk_bytes()),
            )
        } else {
            (
                (current.chunk_entries * 2).min(self.limits.chunk_entries()),
                (current.chunk_bytes * 2).min(self.limits.chunk_bytes()),
            )
        };
        let mut entries = chunk_entries;
        let mut reserved = chunk_bytes.max(bytes);
        let fact_cap = self.limits.fact_actions as u64;
        if ledger.facts.saturating_add(entries) > fact_cap
            || ledger.bytes.saturating_add(reserved) > self.limits.bytes
        {
            ledger = self.evict(&Need {
                seen: ledger,
                evidence: 0,
                facts: entries,
                bytes: reserved,
                keep: None,
            })?;
            // Near the cap with nothing left to evict, reserve what fits.
            entries = entries.min(fact_cap.saturating_sub(ledger.facts));
            reserved = reserved.min(self.limits.bytes.saturating_sub(ledger.bytes));
            if entries < 1 || reserved < bytes {
                self.reservation.set(Reservation {
                    epoch: None,
                    entries: 0,
                    bytes: 0,
                    used_entries: 0,
                    used_bytes: 0,
                    ..current
                });
                return Err(io::Error::new(
                    io::ErrorKind::StorageFull,
                    "optional cache capacity exhausted",
                ));
            }
        }
        let ledger = ledger.with(Class::Facts, entries as i64, reserved as i64);
        self.write_ledger(ledger, true)?;
        self.reservation.set(Reservation {
            epoch: Some(ledger.epoch),
            entries,
            bytes: reserved,
            used_entries: 0,
            used_bytes: 0,
            chunk_entries,
            chunk_bytes,
        });
        Ok(())
    }

    /// Returns the unspent reservation, waiting at most `wait` for the lock.
    /// Whatever happens the handle forgets it: a skipped return over-states,
    /// and a skipped re-add after a recount under-states by at most one
    /// chunk. A missing or pending ledger is left for the next recount.
    fn return_reservation(&self, wait: Duration) {
        let current = self.reservation.get();
        let Some(epoch) = current.epoch else {
            return;
        };
        self.reservation.set(Reservation {
            epoch: None,
            entries: 0,
            bytes: 0,
            used_entries: 0,
            used_bytes: 0,
            ..current
        });
        let _ = self.with_lock_wait(wait, || {
            let Some(ledger) = self.read_ledger()? else {
                return Ok(());
            };
            let next = if ledger.epoch == epoch {
                ledger.with(
                    Class::Facts,
                    -(current.entries as i64),
                    -(current.bytes as i64),
                )
            } else {
                ledger.with(
                    Class::Facts,
                    current.used_entries as i64,
                    current.used_bytes as i64,
                )
            };
            self.write_ledger(next, false)
        });
    }
}

/// A link the file system refuses for want of hard links reports
/// `Unsupported`, which stops fact publication from this handle.
fn link_error(error: io::Error) -> io::Error {
    #[cfg(unix)]
    if matches!(
        error.raw_os_error(),
        Some(libc::EPERM | libc::ENOTSUP | libc::EXDEV)
    ) || error.raw_os_error() == Some(libc::EOPNOTSUPP)
    {
        return io::Error::new(io::ErrorKind::Unsupported, error);
    }
    error
}

fn fresh_epoch() -> io::Result<u64> {
    let mut bytes = [0_u8; 8];
    fill_random(&mut bytes)?;
    Ok(u64::from_le_bytes(bytes))
}

fn remove_present(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

fn is_disk_full(error: &io::Error) -> bool {
    #[cfg(unix)]
    {
        matches!(error.raw_os_error(), Some(libc::ENOSPC | libc::EDQUOT))
    }
    #[cfg(not(unix))]
    {
        let _ = error;
        false
    }
}

/// Every sync in the store goes through here, so tests can count them.
fn sync_file(file: &File) -> io::Result<()> {
    #[cfg(test)]
    test_support::SYNCS.with(|count| count.set(count.get() + 1));
    file.sync_all()
}

fn sync_directories(directories: &HashSet<PathBuf>) -> io::Result<()> {
    for directory in directories {
        sync_file(&File::open(directory)?)?;
    }
    Ok(())
}

fn sync_parent(path: &Path) -> io::Result<()> {
    sync_file(&File::open(
        path.parent().expect("cache entry has a parent"),
    )?)
}

/// A named step of a fact publication. Tests pause a child process there to
/// kill it, or inject a fault; otherwise it does nothing.
#[cfg(test)]
fn test_point(step: &str, path: &Path) -> io::Result<()> {
    test_support::point(step, path)
}

#[cfg(not(test))]
#[inline(always)]
fn test_point(_step: &str, _path: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod test_support {
    use std::{
        cell::{Cell, RefCell},
        io,
        path::Path,
        time::Duration,
    };

    type Hook = Box<dyn FnMut(&str, &Path) -> io::Result<()>>;

    thread_local! {
        pub static SYNCS: Cell<u64> = const { Cell::new(0) };
        pub static INVENTORIES: Cell<u64> = const { Cell::new(0) };
        pub static EVICTIONS: Cell<u64> = const { Cell::new(0) };
        pub static HOOK: RefCell<Option<Hook>> = const { RefCell::new(None) };
    }

    pub fn point(step: &str, path: &Path) -> io::Result<()> {
        if std::env::var("GEOSPEC_CACHE_PAUSE_AT").is_ok_and(|at| at == step) {
            let gate = std::env::var("GEOSPEC_CACHE_PAUSE_GATE").expect("pause gate");
            std::fs::write(gate, b"paused")?;
            loop {
                std::thread::sleep(Duration::from_secs(1));
            }
        }
        HOOK.with(|hook| match hook.borrow_mut().as_mut() {
            Some(hook) => hook(step, path),
            None => Ok(()),
        })
    }
}

fn load_or_create_secret(root: &Path) -> io::Result<[u8; SECRET_BYTES]> {
    let authority = root.join("authority");
    ensure_plain_directory(&authority)?;
    if !fs::canonicalize(&authority)?.starts_with(root) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "cache authority escaped root",
        ));
    }
    let key = authority.join("install-secret-v1");
    match read_bounded(&key, SECRET_BYTES) {
        Ok(bytes) => {
            return bytes
                .try_into()
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid cache secret"));
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let mut secret = [0_u8; SECRET_BYTES];
    fill_random(&mut secret)?;
    let mut suffix = [0_u8; 8];
    fill_random(&mut suffix)?;
    let temporary = authority.join(format!(".install-secret-{}", hex(&suffix)));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temporary)?;
    file.write_all(&secret)?;
    sync_file(&file)?;
    match fs::hard_link(&temporary, &key) {
        Ok(()) => sync_file(&File::open(&authority)?)?,
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => {
            let _ = fs::remove_file(&temporary);
            return Err(error);
        }
    }
    let _ = fs::remove_file(&temporary);
    let bytes = read_bounded(&key, SECRET_BYTES)?;
    bytes
        .try_into()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid cache secret"))
}

fn publish_immutable(path: &Path, bytes: &[u8]) -> io::Result<()> {
    publish_immutable_with(path, bytes, |file, bytes| {
        file.write_all(bytes)?;
        sync_file(file)
    })
}

fn publish_immutable_with(
    path: &Path,
    bytes: &[u8],
    write: impl FnOnce(&mut File, &[u8]) -> io::Result<()>,
) -> io::Result<()> {
    let mut suffix = [0_u8; 8];
    fill_random(&mut suffix)?;
    let temporary = path.with_extension(format!("tmp-{}", hex(&suffix)));
    publish_immutable_with_temporary(path, &temporary, bytes, write)
}

fn publish_immutable_with_temporary(
    path: &Path,
    temporary: &Path,
    bytes: &[u8],
    write: impl FnOnce(&mut File, &[u8]) -> io::Result<()>,
) -> io::Result<()> {
    let mut created = false;
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(temporary)?;
        created = true;
        write(&mut file, bytes)?;
        drop(file);
        match fs::hard_link(temporary, path) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                if !read_bounded(path, bytes.len()).is_ok_and(|existing| existing == bytes) {
                    return Err(error);
                }
            }
            Err(error) => return Err(error),
        }
        sync_file(&File::open(
            path.parent().expect("published file has a parent"),
        )?)
    })();
    if created {
        let _ = fs::remove_file(temporary);
    }
    result
}

/// The canonical form `path` will have once created: its deepest existing
/// ancestor canonicalized, then the missing components. A missing component
/// that is `..` or `.` refuses, since it cannot be resolved without creating.
fn prospective_canonical(path: &Path) -> io::Result<PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    let mut missing = Vec::new();
    let mut existing = absolute.as_path();
    loop {
        match fs::canonicalize(existing) {
            Ok(resolved) => {
                let mut result = resolved;
                for component in missing.iter().rev() {
                    result.push(component);
                }
                return Ok(result);
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let name = existing.file_name().ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidInput, "unresolvable cache root")
                })?;
                missing.push(name.to_owned());
                existing = existing.parent().ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidInput, "unresolvable cache root")
                })?;
            }
            Err(error) => return Err(error),
        }
    }
}

fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn ensure_plain_directory(path: &Path) -> io::Result<()> {
    match fs::create_dir(path) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error),
    }
    if !fs::symlink_metadata(path)?.file_type().is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "cache directory is not a plain directory",
        ));
    }
    Ok(())
}

fn valid_address(address: &EvidenceAddress) -> bool {
    valid_digest(&address.action_sha256)
        && valid_digest(&address.producer_profile_sha256)
        && class_of(address.family, address.codec).is_some()
}

fn read_bounded(path: &Path, limit: usize) -> io::Result<Vec<u8>> {
    if !fs::symlink_metadata(path)?.file_type().is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "cache record is not a plain file",
        ));
    }
    let file = File::open(path)?;
    let metadata = file.metadata()?;
    if metadata.len() > limit as u64 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "cache record too large",
        ));
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(limit as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "cache record too large",
        ));
    }
    Ok(bytes)
}

fn domain_id(domain: &[u8], secret: &[u8]) -> String {
    let mut hash = Sha256::new();
    hash.update(domain);
    hash.update(secret);
    format!("sha256:{}", hex(&hash.finalize()))
}

fn fill_random(bytes: &mut [u8]) -> io::Result<()> {
    getrandom::fill(bytes)
        .map_err(|error| io::Error::other(format!("operating-system randomness failed: {error}")))
}

fn decode_hex_32(value: &str) -> Option<[u8; 32]> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let mut result = [0_u8; 32];
    for (index, output) in result.iter_mut().enumerate() {
        *output = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16).ok()?;
    }
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        process::Command,
        time::{Duration, Instant, SystemTime, UNIX_EPOCH},
    };

    fn roots(name: &str) -> (PathBuf, PathBuf) {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let cache = std::env::temp_dir().join(format!(
            "geospec-cache-host-{name}-{}-{nonce}",
            std::process::id()
        ));
        let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let project = manifest.ancestors().nth(4).unwrap().to_path_buf();
        (cache, project)
    }

    fn address() -> EvidenceAddress {
        EvidenceAddress {
            action_sha256: "11".repeat(32),
            family: "native-b48-overlap-evidence-v1",
            codec: "geospec-overlap-evidence-json-v1",
            producer_profile_sha256: "22".repeat(32),
        }
    }

    fn managed(root: &Path) -> PathBuf {
        root.join(MANAGED_DIRECTORY)
    }

    fn action_file(root: &Path, digest: &str, extension: &str) -> PathBuf {
        managed(root)
            .join("actions")
            .join(&digest[..2])
            .join(format!("{digest}.{extension}"))
    }

    fn content_file(root: &Path, digest: &str) -> PathBuf {
        managed(root)
            .join("content")
            .join(&digest[..2])
            .join(format!("{digest}.json"))
    }

    fn fact_file(root: &Path, digest: &str, extension: &str) -> PathBuf {
        managed(root)
            .join("facts")
            .join(&digest[..2])
            .join(format!("{digest}.{extension}"))
    }

    fn fact_at(index: usize) -> EvidenceAddress {
        EvidenceAddress {
            action_sha256: sha256_hex(index.to_string()),
            ..fact("00")
        }
    }

    fn fact(byte: &str) -> EvidenceAddress {
        EvidenceAddress {
            action_sha256: byte.repeat(32),
            family: FACTS_FAMILY,
            codec: "geospec-facts-json-v1",
            producer_profile_sha256: "22".repeat(32),
        }
    }

    fn addressed(byte: &str) -> EvidenceAddress {
        let mut value = address();
        value.action_sha256 = byte.repeat(32);
        value
    }

    fn small_limits() -> Limits {
        Limits {
            actions: 4,
            fact_actions: 8,
            bytes: 4096,
            low_water: 2048,
            slice: 2,
            temporary_age: TEMPORARY_AGE,
        }
    }

    fn disk_ledger(root: &Path) -> Ledger {
        Ledger::decode(&fs::read(managed(root).join("ledger")).unwrap()).unwrap()
    }

    fn inventory_of(cache: &AuthenticatedOverlapCache) -> Inventory {
        cache
            .with_lock_wait(CHECKPOINT_WAIT, || cache.inventory())
            .unwrap()
    }

    /// Counts without the epoch, which every recount changes.
    fn counts(ledger: &Ledger) -> (u64, u64, u64) {
        (ledger.evidence, ledger.facts, ledger.bytes)
    }

    /// The ledger on disk covers the inventory in every dimension.
    fn assert_covers(root: &Path, cache: &AuthenticatedOverlapCache) {
        let ledger = disk_ledger(root);
        let actual = inventory_of(cache).ledger();
        assert!(
            ledger.evidence >= actual.evidence
                && ledger.facts >= actual.facts
                && ledger.bytes >= actual.bytes,
            "ledger {ledger:?} under-states inventory {actual:?}"
        );
    }

    fn set_hook(hook: impl FnMut(&str, &Path) -> io::Result<()> + 'static) {
        test_support::HOOK.with(|slot| *slot.borrow_mut() = Some(Box::new(hook)));
    }

    fn clear_hook() {
        test_support::HOOK.with(|slot| *slot.borrow_mut() = None);
    }

    fn child(mode: &str, root: &Path, project: &Path, gate: &Path) -> Command {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .arg("--exact")
            .arg("tests::cache_process_child")
            .env("GEOSPEC_CACHE_CHILD_MODE", mode)
            .env("GEOSPEC_CACHE_CHILD_ROOT", root)
            .env("GEOSPEC_CACHE_CHILD_PROJECT", project)
            .env("GEOSPEC_CACHE_CHILD_GATE", gate);
        command
    }

    fn wait_for(path: &Path) {
        let deadline = Instant::now() + Duration::from_secs(30);
        while !path.exists() {
            assert!(
                Instant::now() < deadline,
                "{} never appeared",
                path.display()
            );
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    fn temporaries_in(directory: &Path) -> Vec<PathBuf> {
        match fs::read_dir(directory) {
            Ok(entries) => entries
                .map(|entry| entry.unwrap().path())
                .filter(|path| {
                    path.extension()
                        .and_then(|extension| extension.to_str())
                        .is_some_and(|extension| extension.starts_with("tmp-"))
                })
                .collect(),
            Err(_) => Vec::new(),
        }
    }

    #[test]
    fn authentication_digest_and_seal_fail_closed() {
        let (root, project) = roots("integrity");
        let cache = AuthenticatedOverlapCache::open(&root, &project).unwrap();
        let address = address();
        let payload = br#"{"schema":"payload"}"#;
        cache.publish(&address, payload);
        assert_eq!(cache.load(&address).as_deref(), Some(payload.as_slice()));

        let action_path = action_file(&root, &address.action_sha256, "json");
        let original_action = fs::read(&action_path).unwrap();
        let mut action: serde_json::Value = serde_json::from_slice(&original_action).unwrap();
        action["envelope"]["codec"] = serde_json::Value::String("mutated-codec".into());
        fs::write(&action_path, serde_json::to_vec(&action).unwrap()).unwrap();
        assert_eq!(
            cache.load(&address),
            None,
            "mutated envelope authentication"
        );
        fs::write(&action_path, original_action).unwrap();

        let mut second_address = address.clone();
        second_address.action_sha256 = "33".repeat(32);
        cache.publish(&second_address, payload);
        let second_action_path = action_file(&root, &second_address.action_sha256, "json");
        let action: ActionRecord =
            serde_json::from_slice(&fs::read(&second_action_path).unwrap()).unwrap();
        let content_path = content_file(&root, &action.envelope.content_sha256);
        fs::write(content_path, b"mutated payload").unwrap();
        assert_eq!(cache.load(&address), None, "mutated content digest");

        let sealed = cache.flush();
        cache.publish(&address, b"replacement");
        assert_eq!(cache.flush(), sealed, "sealed flush result is stable");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn sealed_store_does_not_publish() {
        let (root, project) = roots("sealed");
        let cache = AuthenticatedOverlapCache::open(&root, &project).unwrap();
        let address = address();
        cache.flush();
        cache.publish(&address, b"payload");
        assert!(!action_file(&root, &address.action_sha256, "json").exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn unknown_family_cannot_poison_managed_inventory() {
        let (root, project) = roots("unknown-family");
        let cache = AuthenticatedOverlapCache::open(&root, &project).unwrap();
        let mut unknown = address();
        unknown.family = "unknown-family";
        cache.publish(&unknown, b"not admitted");
        assert_eq!(cache.load(&unknown), None);
        assert_eq!(cache.diagnostics.borrow().rejected_writes, 1);
        assert_eq!(cache.diagnostics.borrow().io_failures, 0);
        assert_eq!(
            cache
                .with_lock_wait(PUBLISH_WAIT, || cache.inventory())
                .unwrap()
                .entries,
            0
        );
        drop(cache);
        let reopened = AuthenticatedOverlapCache::open(&root, &project).unwrap();
        reopened.publish(&address(), b"known family");
        assert_eq!(
            reopened.load(&address()).as_deref(),
            Some(b"known family".as_slice())
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn clearing_overlap_preserves_exact_fact_and_poison() {
        let (root, project) = roots("family-clear");
        let cache = AuthenticatedOverlapCache::open(&root, &project).unwrap();
        let overlap = address();
        let mut exact = address();
        exact.action_sha256 = "44".repeat(32);
        exact.family = "native-exact-step-clusters-v1";
        exact.codec = "geospec-exact-clusters-json-v1";
        cache.publish(&overlap, b"shared-content");
        cache.publish(&exact, b"shared-content");
        let mut poisoned = address();
        poisoned.action_sha256 = "55".repeat(32);
        cache.publish(&poisoned, b"first");
        cache.publish(&poisoned, b"second");
        let poison = action_file(&root, &poisoned.action_sha256, "poison");
        assert!(poison.exists());

        assert!(cache.clear());
        assert_eq!(cache.load(&overlap), None);
        drop(cache);
        let reopened = AuthenticatedOverlapCache::open(&root, &project).unwrap();
        assert_eq!(
            reopened.load(&exact).as_deref(),
            Some(b"shared-content".as_slice())
        );
        assert!(poison.exists(), "a conflict must stay poisoned after clear");
        assert!(root.join("authority").join("install-secret-v1").exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn clear_collects_a_corrupt_action_and_keeps_other_families() {
        let (root, project) = roots("corrupt-clear");
        let cache = AuthenticatedOverlapCache::open(&root, &project).unwrap();
        let overlap = address();
        let mut exact = address();
        exact.action_sha256 = "44".repeat(32);
        exact.family = "native-exact-step-clusters-v1";
        exact.codec = "geospec-exact-clusters-json-v1";
        cache.publish(&overlap, b"overlap");
        cache.publish(&exact, b"exact");
        cache.publish(&fact("aa"), b"fact");
        let corrupt = action_file(&root, &exact.action_sha256, "json");
        fs::write(&corrupt, b"invalid").unwrap();
        assert_eq!(cache.load(&exact), None);
        assert!(cache.clear());
        assert_eq!(cache.load(&overlap), None);
        assert_eq!(cache.load(&exact), None);
        assert!(!corrupt.exists());
        assert_eq!(cache.load(&fact("aa")).as_deref(), Some(b"fact".as_slice()));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn interrupted_clear_epoch_fences_old_publisher_without_acknowledging_clear() {
        let (root, project) = roots("interrupted-clear");
        let old = AuthenticatedOverlapCache::open(&root, &project).unwrap();
        let overlap = address();
        old.publish(&overlap, b"old result");
        // Model process death after the durable generation update but before
        // the first action deletion. The interrupted clear has no ack.
        old.with_lock_wait(PUBLISH_WAIT, || old.write_generation(1))
            .unwrap();
        old.publish(&overlap, b"stale computation");
        assert_eq!(
            old.load(&overlap).as_deref(),
            Some(b"old result".as_slice())
        );
        drop(old);
        let reopened = AuthenticatedOverlapCache::open(&root, &project).unwrap();
        assert!(
            reopened.clear(),
            "a later explicit clear completes deletion"
        );
        assert_eq!(reopened.load(&overlap), None);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn completed_family_reopens_in_fresh_process() {
        let (root, project) = roots("fresh-process");
        let mut address = address();
        address.family = "native-exact-step-clusters-v1";
        address.codec = "geospec-exact-clusters-json-v1";
        let cache = AuthenticatedOverlapCache::open(&root, &project).unwrap();
        cache.publish(&address, b"completed-fact");
        drop(cache);

        let status = Command::new(std::env::current_exe().unwrap())
            .arg("--exact")
            .arg("tests::cache_reopen_child")
            .env("GEOSPEC_CACHE_REOPEN_ROOT", &root)
            .env("GEOSPEC_CACHE_REOPEN_PROJECT", &project)
            .status()
            .unwrap();
        assert!(status.success());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn cache_reopen_child() {
        let Ok(root) = std::env::var("GEOSPEC_CACHE_REOPEN_ROOT") else {
            return;
        };
        let project = std::env::var("GEOSPEC_CACHE_REOPEN_PROJECT").unwrap();
        let mut address = address();
        address.family = "native-exact-step-clusters-v1";
        address.codec = "geospec-exact-clusters-json-v1";
        let cache = AuthenticatedOverlapCache::open(root, project).unwrap();
        assert_eq!(
            cache.load(&address).as_deref(),
            Some(b"completed-fact".as_slice())
        );
    }

    #[test]
    fn rejects_unsafe_digests_and_poisoned_divergent_action() {
        let (root, project) = roots("conflict");
        let cache = AuthenticatedOverlapCache::open(&root, &project).unwrap();
        let address = address();
        cache.publish(&address, b"one");
        assert_eq!(cache.load(&address).as_deref(), Some(b"one".as_slice()));

        let mut invalid = address.clone();
        invalid.action_sha256 = "../outside".into();
        cache.publish(&invalid, b"unsafe");
        assert_eq!(cache.load(&invalid), None);
        invalid.action_sha256 = "AA".repeat(32);
        cache.publish(&invalid, b"uppercase");
        assert_eq!(cache.load(&invalid), None);
        invalid = address.clone();
        invalid.producer_profile_sha256 = "bb/../".into();
        cache.publish(&invalid, b"unsafe producer");
        assert_eq!(cache.load(&invalid), None);

        cache.publish(&address, b"different");
        assert_eq!(cache.load(&address), None, "divergent action is poisoned");
        assert!(action_file(&root, &address.action_sha256, "poison").exists());
        drop(cache);
        let reopened = AuthenticatedOverlapCache::open(&root, &project).unwrap();
        assert_eq!(reopened.load(&address), None, "poison survives reopen");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn concurrent_divergent_writers_poison_action() {
        let (root, project) = roots("concurrent-conflict");
        let first = AuthenticatedOverlapCache::open(&root, &project).unwrap();
        let second = AuthenticatedOverlapCache::open(&root, &project).unwrap();
        let address = address();
        let gate = std::sync::Barrier::new(3);
        std::thread::scope(|scope| {
            let first_gate = &gate;
            let first_address = &address;
            scope.spawn(move || {
                first_gate.wait();
                first.publish(first_address, b"first");
            });
            let second_gate = &gate;
            let second_address = &address;
            scope.spawn(move || {
                second_gate.wait();
                second.publish(second_address, b"second");
            });
            gate.wait();
        });
        let reopened = AuthenticatedOverlapCache::open(&root, &project).unwrap();
        // A nonblocking lock may refuse one simultaneous optional write. Once
        // the lock is free, a divergent retry must poison the surviving fact.
        match reopened.load(&address).as_deref() {
            Some(b"first") => reopened.publish(&address, b"second"),
            Some(b"second") => reopened.publish(&address, b"first"),
            None => {}
            _ => panic!("unexpected concurrent cache payload"),
        }
        assert_eq!(reopened.load(&address), None);
        assert!(action_file(&root, &address.action_sha256, "poison").exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn legacy_records_are_untouched_and_managed_clear_is_family_scoped() {
        let (root, project) = roots("legacy");
        fs::create_dir_all(root.join("actions")).unwrap();
        fs::create_dir_all(root.join("content")).unwrap();
        let legacy = root
            .join("actions")
            .join(format!("{}.json", address().action_sha256));
        fs::write(&legacy, b"legacy bytes").unwrap();
        let cache = AuthenticatedOverlapCache::open(&root, &project).unwrap();
        assert_eq!(cache.load(&address()), None, "legacy actions safely miss");
        let exact = EvidenceAddress {
            family: EXACT_FAMILY,
            codec: "geospec-exact-clusters-json-v1",
            ..addressed("44")
        };
        cache.publish(&address(), b"overlap");
        cache.publish(&exact, b"exact");
        assert!(cache.clear());
        assert_eq!(cache.load(&address()), None);
        assert_eq!(cache.load(&exact).as_deref(), Some(b"exact".as_slice()));
        assert_eq!(fs::read(legacy).unwrap(), b"legacy bytes");
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn managed_symlinks_refuse_before_following_them() {
        use std::os::unix::fs::symlink;
        let (root, project) = roots("symlink");
        let (outside, _) = roots("outside");
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&outside).unwrap();
        symlink(&outside, managed(&root)).unwrap();
        assert!(AuthenticatedOverlapCache::open(&root, &project).is_err());
        assert_eq!(fs::read_dir(&outside).unwrap().count(), 0);
        fs::remove_file(managed(&root)).unwrap();
        fs::create_dir_all(managed(&root)).unwrap();
        symlink(&outside, managed(&root).join("actions")).unwrap();
        assert!(AuthenticatedOverlapCache::open(&root, &project).is_err());
        assert_eq!(fs::read_dir(&outside).unwrap().count(), 0);
        fs::remove_dir_all(root).unwrap();
        let (authority_root, _) = roots("authority-symlink");
        fs::create_dir_all(&authority_root).unwrap();
        symlink(&outside, authority_root.join("authority")).unwrap();
        assert!(AuthenticatedOverlapCache::open(&authority_root, &project).is_err());
        assert_eq!(fs::read_dir(&outside).unwrap().count(), 0);
        fs::remove_dir_all(authority_root).unwrap();
        fs::remove_dir_all(outside).unwrap();
    }

    #[test]
    fn cap_pressure_preserves_shared_content_and_divergence_poison() {
        let (root, project) = roots("pressure");
        let cache =
            AuthenticatedOverlapCache::open_with_limits(&root, &project, small_limits()).unwrap();
        let first = addressed("11");
        let second = addressed("22");
        cache.publish(&first, b"shared");
        cache.publish(&second, b"shared");
        cache.publish(&addressed("33"), b"third");
        cache.publish(&addressed("44"), b"fourth");
        let before_repeat = cache
            .with_lock_wait(PUBLISH_WAIT, || cache.inventory())
            .unwrap()
            .entries;
        cache.publish(&second, b"shared");
        assert_eq!(
            cache
                .with_lock_wait(PUBLISH_WAIT, || cache.inventory())
                .unwrap()
                .entries,
            before_repeat
        );
        // Conflict handling precedes capacity collection: it must not evict
        // this target and then publish divergent bytes as a fresh action.
        cache.publish(&second, b"different");
        assert_eq!(cache.load(&second), None);
        assert!(action_file(&root, &second.action_sha256, "poison").exists());
        assert_eq!(cache.load(&first).as_deref(), Some(b"shared".as_slice()));
        cache.publish(&addressed("55"), b"fifth");
        let inventory = cache
            .with_lock_wait(PUBLISH_WAIT, || cache.inventory())
            .unwrap();
        // Poison markers are permanent and never count toward the cap.
        assert!(inventory.actions.len() <= small_limits().actions);
        assert!(inventory.bytes <= small_limits().bytes);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn byte_pressure_reclaims_only_unreferenced_managed_content() {
        let (root, project) = roots("byte-pressure");
        let cache =
            AuthenticatedOverlapCache::open_with_limits(&root, &project, small_limits()).unwrap();
        let first = addressed("11");
        let second = addressed("22");
        let first_payload = vec![b'x'; 3000];
        let second_payload = vec![b'y'; 3000];
        cache.publish(&first, &first_payload);
        assert_eq!(
            cache.load(&first).as_deref(),
            Some(first_payload.as_slice())
        );
        cache.publish(&second, &second_payload);
        assert_eq!(
            cache.load(&second).as_deref(),
            Some(second_payload.as_slice())
        );
        assert_eq!(cache.load(&first), None);
        let inventory = cache
            .with_lock_wait(PUBLISH_WAIT, || cache.inventory())
            .unwrap();
        assert!(inventory.bytes <= small_limits().bytes);
        assert_eq!(inventory.content.len(), 1);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    #[ignore = "manual loaded-host inventory cost diagnostic"]
    fn near_cap_cold_publish_and_hit_cost_diagnostic() {
        let (root, project) = roots("cost");
        let cache = AuthenticatedOverlapCache::open(&root, &project).unwrap();
        for index in 1..MAX_EVIDENCE_ACTIONS {
            let mut item = address();
            item.action_sha256 = format!("{index:064x}");
            cache.publish(&item, b"shared");
        }
        let item = addressed("ff");
        let start = Instant::now();
        cache.publish(&item, b"shared");
        let publish = start.elapsed();
        let start = Instant::now();
        assert_eq!(cache.load(&item).as_deref(), Some(b"shared".as_slice()));
        let hit = start.elapsed();
        eprintln!(
            "near-cap managed publish={publish:?} hit={hit:?} actions={MAX_EVIDENCE_ACTIONS}"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    #[ignore = "manual facts publish cost diagnostic; writes about 90,000 small files"]
    fn facts_publish_cost_is_flat_in_store_size_diagnostic() {
        let (root, project) = roots("facts-cost");
        let cache = AuthenticatedOverlapCache::open(&root, &project).unwrap();
        let payload = |index: usize| format!("{index:0400}").into_bytes();
        const TOTAL: usize = 64_000;
        const SLICE: usize = 8_000;
        let mut per_slice = Vec::new();
        let start = Instant::now();
        let mut mark = start;
        for index in 0..TOTAL {
            cache.publish(&fact_at(index), &payload(index));
            if index % SLICE == SLICE - 1 {
                per_slice.push(mark.elapsed().as_secs_f64() * 1e6 / SLICE as f64);
                mark = Instant::now();
            }
        }
        let publish = start.elapsed().as_secs_f64() * 1e6 / TOTAL as f64;
        let start = Instant::now();
        for index in 0..TOTAL {
            assert_eq!(cache.load(&fact_at(index)), Some(payload(index)));
        }
        let load = start.elapsed().as_secs_f64() * 1e6 / TOTAL as f64;
        // Past the 65,536-fact cap: the slowest publication holds the one
        // eviction pass, inventory included.
        let mut slowest = Duration::ZERO;
        let evictions = test_support::EVICTIONS.with(Cell::get);
        for index in TOTAL..TOTAL + 4_000 {
            let start = Instant::now();
            cache.publish(&fact_at(index), &payload(index));
            slowest = slowest.max(start.elapsed());
        }
        let evictions = test_support::EVICTIONS.with(Cell::get) - evictions;
        assert_eq!(cache.diagnostics.borrow().rejected_writes, 0);
        assert_eq!(cache.diagnostics.borrow().io_failures, 0);
        drop(cache);
        fs::remove_dir_all(&root).unwrap();

        // 11 writers × 2,000 publications into one fresh store.
        let (root, project) = roots("facts-cost-writers");
        AuthenticatedOverlapCache::open(&root, &project).unwrap();
        let start = Instant::now();
        let rejected: u64 = std::thread::scope(|scope| {
            let handles: Vec<_> = (0..11)
                .map(|writer| {
                    let (root, project) = (&root, &project);
                    scope.spawn(move || {
                        let cache = AuthenticatedOverlapCache::open(root, project).unwrap();
                        for index in 0..2_000 {
                            let key = 1_000_000 + writer * 2_000 + index;
                            cache.publish(&fact_at(key), &payload(key));
                        }
                        let diagnostics = cache.diagnostics.borrow();
                        diagnostics.rejected_writes + diagnostics.io_failures
                    })
                })
                .collect();
            handles
                .into_iter()
                .map(|handle| handle.join().unwrap())
                .sum()
        });
        let writers = start.elapsed();
        let cache = AuthenticatedOverlapCache::open(&root, &project).unwrap();
        assert_eq!(inventory_of(&cache).ledger().facts, 22_000);
        eprintln!(
            "facts publish µs per publication by {SLICE}: {per_slice:.1?}; mean {publish:.1} µs; \
             load {load:.1} µs; {evictions} eviction(s) past the cap, slowest publication \
             {slowest:?}; 11 writers × 2,000 in {writers:?}, {rejected} refused"
        );
        assert_eq!(rejected, 0);
        assert!(publish <= 100.0, "{publish} µs per publication");
        let flat = per_slice[per_slice.len() - 1] / per_slice[0];
        assert!(
            flat <= 1.2,
            "cost grew {flat:.2}× from the first slice to the last"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn unknown_entries_are_skipped_and_invalid_actions_collected() {
        let (root, project) = roots("unknown");
        let cache = AuthenticatedOverlapCache::open(&root, &project).unwrap();
        cache.publish(&fact("aa"), b"known");
        cache.publish(&addressed("dd"), b"lacks content");
        let unknown = managed(&root).join("facts").join("unknown.dat");
        fs::write(&unknown, b"foreign").unwrap();
        let foreign = managed(&root).join("facts").join("aa").join("notes.txt");
        fs::write(&foreign, b"foreign").unwrap();
        let corrupt = fact_file(&root, &fact("cc").action_sha256, "fact");
        fs::create_dir_all(corrupt.parent().unwrap()).unwrap();
        fs::write(&corrupt, b"{}").unwrap();
        fs::remove_file(content_file(&root, &sha256_hex(b"lacks content"))).unwrap();
        // Reads of the damaged entries miss.
        assert_eq!(cache.load(&fact("cc")), None);
        assert_eq!(cache.load(&addressed("dd")), None);
        // Recovery runs: unknown entries stay, invalid entries are collected.
        fs::remove_file(managed(&root).join("ledger")).unwrap();
        drop(cache);
        let reopened = AuthenticatedOverlapCache::open(&root, &project).unwrap();
        assert_eq!(
            reopened.load(&fact("aa")).as_deref(),
            Some(b"known".as_slice())
        );
        assert_eq!(reopened.load(&fact("cc")), None);
        assert_eq!(reopened.load(&addressed("dd")), None);
        assert!(!corrupt.exists());
        assert!(!action_file(&root, &addressed("dd").action_sha256, "json").exists());
        assert_eq!(fs::read(&unknown).unwrap(), b"foreign");
        assert_eq!(fs::read(&foreign).unwrap(), b"foreign");
        // Eviction and clear still work with the unknown entries present.
        assert!(reopened.clear());
        reopened.publish(&fact("ee"), b"after");
        assert_eq!(
            reopened.load(&fact("ee")).as_deref(),
            Some(b"after".as_slice())
        );
        // Returning the reservation leaves the ledger exactly at the store.
        reopened.flush();
        assert_eq!(
            counts(&disk_ledger(&root)),
            counts(&inventory_of(&reopened).ledger())
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn republished_orphan_content_is_counted() {
        let (root, project) = roots("orphan-recount");
        let cache = AuthenticatedOverlapCache::open(&root, &project).unwrap();
        let orphan = content_file(&root, &sha256_hex(b"blob"));
        fs::create_dir_all(orphan.parent().unwrap()).unwrap();
        fs::write(&orphan, b"blob").unwrap();
        // A pending ledger makes the publication collect the orphan first.
        let ledger_path = managed(&root).join("ledger");
        fs::write(&ledger_path, b"geospec-cache-ledger-v2 0 0 0 0 pending\n").unwrap();
        cache.publish(&addressed("aa"), b"blob");
        assert_eq!(
            cache.load(&addressed("aa")).as_deref(),
            Some(b"blob".as_slice())
        );
        assert_eq!(
            counts(&disk_ledger(&root)),
            counts(&inventory_of(&cache).ledger())
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn failed_publication_leaves_a_clean_ledger_that_only_over_states() {
        let (root, project) = roots("failed-publish");
        let cache = AuthenticatedOverlapCache::open(&root, &project).unwrap();
        cache.publish(&addressed("aa"), b"one");
        // A file where the content shard belongs makes the write fail after
        // the pending ledger.
        let blocked = content_file(&root, &sha256_hex(b"two"));
        let shard = blocked.parent().unwrap();
        if shard.exists() {
            fs::remove_dir_all(shard).unwrap();
        }
        fs::write(shard, b"not a directory").unwrap();
        cache.publish(&addressed("bb"), b"two");
        assert_eq!(cache.load(&addressed("bb")), None);
        let ledger = disk_ledger(&root);
        let actual = inventory_of(&cache).ledger();
        assert!(!ledger.pending);
        assert!(ledger.evidence >= actual.evidence && ledger.bytes >= actual.bytes);
        assert!(ledger.evidence > actual.evidence);
        fs::remove_file(shard).unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn corrupt_shared_content_is_replaced_without_poisoning_new_actions() {
        let (root, project) = roots("corrupt-content");
        let cache = AuthenticatedOverlapCache::open(&root, &project).unwrap();
        cache.publish(&addressed("aa"), b"shared");
        let blob = content_file(&root, &sha256_hex(b"shared"));
        fs::remove_file(&blob).unwrap();
        fs::write(&blob, b"corrupt").unwrap();
        assert_eq!(cache.load(&addressed("aa")), None);
        cache.publish(&addressed("bb"), b"shared");
        assert!(!action_file(&root, &addressed("bb").action_sha256, "poison").exists());
        for byte in ["bb", "aa"] {
            assert_eq!(
                cache.load(&addressed(byte)).as_deref(),
                Some(b"shared".as_slice())
            );
        }
        cache.publish(&addressed("cc"), b"shared");
        assert_eq!(
            cache.load(&addressed("cc")).as_deref(),
            Some(b"shared".as_slice())
        );
        assert_eq!(cache.diagnostics.borrow().rejected_writes, 0);
        assert_eq!(cache.diagnostics.borrow().io_failures, 0);
        let ledger = disk_ledger(&root);
        let actual = inventory_of(&cache).ledger();
        assert!(
            !ledger.pending && ledger.evidence == actual.evidence && ledger.bytes >= actual.bytes
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn oversized_ledger_is_recounted() {
        let (root, project) = roots("ledger-oversized");
        let cache = AuthenticatedOverlapCache::open(&root, &project).unwrap();
        cache.publish(&fact("aa"), b"one");
        cache.flush();
        let ledger_path = managed(&root).join("ledger");
        fs::write(&ledger_path, vec![b'9'; 4096]).unwrap();
        drop(cache);
        let reopened = AuthenticatedOverlapCache::open(&root, &project).unwrap();
        reopened.publish(&fact("bb"), b"two");
        assert_eq!(
            reopened.load(&fact("bb")).as_deref(),
            Some(b"two".as_slice())
        );
        reopened.flush();
        assert_eq!(
            counts(&disk_ledger(&root)),
            counts(&inventory_of(&reopened).ledger())
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn evidence_and_fact_with_one_digest_are_independent() {
        let (root, project) = roots("one-digest");
        let cache = AuthenticatedOverlapCache::open(&root, &project).unwrap();
        // A facts address whose digest an evidence action already holds.
        cache.publish(&addressed("aa"), b"evidence");
        cache.publish(&fact("aa"), b"fact");
        assert!(!action_file(&root, &addressed("aa").action_sha256, "poison").exists());
        assert!(!fact_file(&root, &fact("aa").action_sha256, "poison").exists());
        assert_eq!(
            cache.load(&addressed("aa")).as_deref(),
            Some(b"evidence".as_slice())
        );
        assert_eq!(cache.load(&fact("aa")).as_deref(), Some(b"fact".as_slice()));
        cache.flush();
        assert_eq!(
            counts(&disk_ledger(&root)),
            counts(&inventory_of(&cache).ledger())
        );
        assert_eq!(
            (disk_ledger(&root).evidence, disk_ledger(&root).facts),
            (1, 1)
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn refused_root_inside_the_project_creates_nothing() {
        let (root, _) = roots("inside-project");
        let project = root.join("project");
        fs::create_dir_all(&project).unwrap();
        let inside = project.join("nested").join("store");
        assert!(AuthenticatedOverlapCache::open(&inside, &project).is_err());
        assert!(!project.join("nested").exists());
        assert!(AuthenticatedOverlapCache::open(&root, &project).is_err());
        assert!(!root.join("authority").exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn facts_only_view_serves_and_writes_only_the_facts_family() {
        let (root, project) = roots("facts-only");
        let cache = Rc::new(AuthenticatedOverlapCache::open(&root, &project).unwrap());
        cache.publish(&address(), b"overlap");
        let view = FactsOnlyCache::new(cache.clone());
        assert_eq!(view.load(&address()), None);
        view.publish(&addressed("33"), b"not stored");
        assert!(!action_file(&root, &addressed("33").action_sha256, "json").exists());
        view.publish(&fact("aa"), b"fact");
        assert_eq!(view.load(&fact("aa")).as_deref(), Some(b"fact".as_slice()));
        assert_eq!(cache.load(&fact("aa")).as_deref(), Some(b"fact".as_slice()));
        drop(view);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn clear_generation_fences_old_miss_publisher_but_not_exact_family() {
        let (root, project) = roots("generation");
        let old = AuthenticatedOverlapCache::open(&root, &project).unwrap();
        let clearer = AuthenticatedOverlapCache::open(&root, &project).unwrap();
        let overlap = address();
        assert_eq!(old.load(&overlap), None);
        assert!(clearer.clear());
        old.publish(&overlap, b"stale result");
        assert_eq!(clearer.load(&overlap), None);
        assert_eq!(old.diagnostics.borrow().rejected_writes, 1);
        let exact = EvidenceAddress {
            family: EXACT_FAMILY,
            codec: "geospec-exact-clusters-json-v1",
            ..addressed("44")
        };
        old.publish(&exact, b"exact result");
        assert_eq!(
            clearer.load(&exact).as_deref(),
            Some(b"exact result".as_slice())
        );
        clearer.publish(&overlap, b"new result");
        assert_eq!(
            old.load(&overlap).as_deref(),
            Some(b"new result".as_slice())
        );
        fs::remove_dir_all(root).unwrap();
    }

    /// One CI worker: publish every shared fact and its own, return the
    /// reservation, wait for the others, then read every fact any worker
    /// published.
    fn run_worker(root: &Path, project: &Path, gate: &Path, index: usize, workers: usize) {
        let cache = AuthenticatedOverlapCache::open(root, project).unwrap();
        let shared = |key: usize| (fact_at(key), format!("shared-{key}").into_bytes());
        let own = |worker: usize, key: usize| {
            (
                fact_at(4096 + worker * 64 + key),
                format!("worker-{worker}-{key}").into_bytes(),
            )
        };
        for key in 0..64 {
            let (address, payload) = shared(key);
            if cache.load(&address).is_none() {
                cache.publish(&address, &payload);
            }
            let (address, payload) = own(index, key);
            cache.publish(&address, &payload);
        }
        cache.flush();
        fs::write(gate.with_extension(format!("{index}.ready")), b"ready").unwrap();
        let deadline = Instant::now() + Duration::from_secs(60);
        while (0..workers).any(|other| !gate.with_extension(format!("{other}.ready")).exists()) {
            assert!(
                Instant::now() < deadline,
                "another worker never became ready"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        for key in 0..64 {
            let (address, payload) = shared(key);
            assert_eq!(cache.load(&address), Some(payload), "shared fact {key}");
            for other in 0..workers {
                let (address, payload) = own(other, key);
                assert_eq!(
                    cache.load(&address),
                    Some(payload),
                    "worker {other} fact {key}"
                );
            }
        }
        let diagnostics = cache.diagnostics.borrow();
        assert_eq!(diagnostics.rejected_writes, 0, "no publication refused");
        assert_eq!(diagnostics.io_failures, 0);
    }

    #[test]
    fn cache_process_child() {
        let Ok(mode) = std::env::var("GEOSPEC_CACHE_CHILD_MODE") else {
            return;
        };
        let root = PathBuf::from(std::env::var("GEOSPEC_CACHE_CHILD_ROOT").unwrap());
        let project = PathBuf::from(std::env::var("GEOSPEC_CACHE_CHILD_PROJECT").unwrap());
        let gate = PathBuf::from(std::env::var("GEOSPEC_CACHE_CHILD_GATE").unwrap());
        let name = std::env::var("GEOSPEC_CACHE_CHILD_NAME").unwrap_or_default();
        if mode == "worker" {
            let workers = std::env::var("GEOSPEC_CACHE_CHILD_WORKERS")
                .unwrap()
                .parse()
                .unwrap();
            run_worker(&root, &project, &gate, name.parse().unwrap(), workers);
            return;
        }
        let cache = AuthenticatedOverlapCache::open(&root, &project).unwrap();
        if mode == "publish" {
            fs::write(gate.with_extension(format!("{name}.ready")), b"ready").unwrap();
            while !gate.exists() {
                std::thread::sleep(Duration::from_millis(1));
            }
            // Both producers start together. If the optional bounded lock
            // refuses one, retry after the winner exits so both publications
            // are actually admitted and their differing bytes are compared.
            for _ in 0..5 {
                cache.publish(&address(), name.as_bytes());
                if cache.diagnostics.borrow().writes > 0
                    || action_file(&root, &address().action_sha256, "poison").exists()
                {
                    return;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            panic!("simultaneous publisher never admitted");
        } else if mode == "fact-publish" {
            // Lock-free: no retry is needed for both to be compared.
            fs::write(gate.with_extension(format!("{name}.ready")), b"ready").unwrap();
            while !gate.exists() {
                std::thread::sleep(Duration::from_millis(1));
            }
            cache.publish(&fact("ab"), name.as_bytes());
        } else if mode == "fact" {
            // Pauses inside the publication when GEOSPEC_CACHE_PAUSE_AT names a step.
            cache.publish(&fact("ab"), b"paused");
        } else if mode == "reserve" {
            for index in 0..10 {
                cache.publish(&fact_at(9_000 + index), b"reserved");
            }
            fs::write(&gate, b"reserved").unwrap();
            std::thread::sleep(Duration::from_secs(30));
        } else if mode == "stale" {
            assert_eq!(cache.load(&address()), None);
            fs::write(&gate, b"missed").unwrap();
            while !gate.with_extension("go").exists() {
                std::thread::sleep(Duration::from_millis(1));
            }
            cache.publish(&address(), b"stale computation");
        } else if mode == "hold" || mode == "orphan" {
            cache
                .with_lock_wait(PUBLISH_WAIT, || {
                    if mode == "orphan" {
                        // The publication order: pending ledger, then blob.
                        let bytes = b"orphan blob";
                        let ledger = cache.ledger()?;
                        cache.write_ledger(
                            Ledger {
                                pending: true,
                                ..ledger
                            },
                            true,
                        )?;
                        let path = content_file(&root, &sha256_hex(bytes));
                        ensure_plain_directory(path.parent().unwrap())?;
                        publish_immutable(&path, bytes)?;
                    }
                    fs::write(&gate, b"locked")?;
                    std::thread::sleep(Duration::from_secs(30));
                    Ok(())
                })
                .unwrap();
        }
    }

    #[cfg(unix)]
    #[test]
    fn two_processes_poison_admitted_divergence_and_busy_lock_refuses() {
        let (root, project) = roots("process-race");
        AuthenticatedOverlapCache::open(&root, &project).unwrap();
        let gate = root.join("start");
        let child = |name: &str| {
            Command::new(std::env::current_exe().unwrap())
                .arg("--exact")
                .arg("tests::cache_process_child")
                .env("GEOSPEC_CACHE_CHILD_MODE", "publish")
                .env("GEOSPEC_CACHE_CHILD_ROOT", &root)
                .env("GEOSPEC_CACHE_CHILD_PROJECT", &project)
                .env("GEOSPEC_CACHE_CHILD_GATE", &gate)
                .env("GEOSPEC_CACHE_CHILD_NAME", name)
                .spawn()
                .unwrap()
        };
        let mut first = child("first");
        let mut second = child("second");
        let deadline = Instant::now() + Duration::from_secs(5);
        while (!gate.with_extension("first.ready").exists()
            || !gate.with_extension("second.ready").exists())
            && Instant::now() < deadline
        {
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(
            gate.with_extension("first.ready").exists()
                && gate.with_extension("second.ready").exists()
        );
        fs::write(&gate, b"go").unwrap();
        assert!(first.wait().unwrap().success());
        assert!(second.wait().unwrap().success());
        let cache = AuthenticatedOverlapCache::open(&root, &project).unwrap();
        assert_eq!(cache.load(&address()), None);
        assert!(action_file(&root, &address().action_sha256, "poison").exists());

        fs::remove_file(&gate).unwrap();
        let mut holder = Command::new(std::env::current_exe().unwrap())
            .arg("--exact")
            .arg("tests::cache_process_child")
            .env("GEOSPEC_CACHE_CHILD_MODE", "hold")
            .env("GEOSPEC_CACHE_CHILD_ROOT", &root)
            .env("GEOSPEC_CACHE_CHILD_PROJECT", &project)
            .env("GEOSPEC_CACHE_CHILD_GATE", &gate)
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !gate.exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(gate.exists());
        let other = addressed("22");
        cache.publish(&other, b"uncommitted");
        assert_eq!(cache.load(&other), None);
        assert!(!cache.clear());
        assert_eq!(cache.diagnostics.borrow().writes, 0);
        assert!(cache.diagnostics.borrow().rejected_writes >= 1);
        holder.kill().unwrap();
        holder.wait().unwrap();
        assert_eq!(cache.load(&other), None);
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn killed_blob_before_action_is_reclaimed_without_a_false_hit() {
        let (root, project) = roots("orphan-kill");
        AuthenticatedOverlapCache::open(&root, &project).unwrap();
        let gate = root.join("orphan-ready");
        let mut child = Command::new(std::env::current_exe().unwrap())
            .arg("--exact")
            .arg("tests::cache_process_child")
            .env("GEOSPEC_CACHE_CHILD_MODE", "orphan")
            .env("GEOSPEC_CACHE_CHILD_ROOT", &root)
            .env("GEOSPEC_CACHE_CHILD_PROJECT", &project)
            .env("GEOSPEC_CACHE_CHILD_GATE", &gate)
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !gate.exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(gate.exists());
        child.kill().unwrap();
        child.wait().unwrap();
        let orphan = content_file(&root, &sha256_hex(b"orphan blob"));
        assert!(orphan.exists());
        let reopened = AuthenticatedOverlapCache::open(&root, &project).unwrap();
        assert!(!orphan.exists());
        assert_eq!(reopened.load(&address()), None);
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn second_process_clear_fences_first_process_miss_then_publish() {
        let (root, project) = roots("process-clear");
        AuthenticatedOverlapCache::open(&root, &project).unwrap();
        let gate = root.join("miss-ready");
        let mut child = Command::new(std::env::current_exe().unwrap())
            .arg("--exact")
            .arg("tests::cache_process_child")
            .env("GEOSPEC_CACHE_CHILD_MODE", "stale")
            .env("GEOSPEC_CACHE_CHILD_ROOT", &root)
            .env("GEOSPEC_CACHE_CHILD_PROJECT", &project)
            .env("GEOSPEC_CACHE_CHILD_GATE", &gate)
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !gate.exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(gate.exists());
        let clearer = AuthenticatedOverlapCache::open(&root, &project).unwrap();
        assert!(clearer.clear());
        fs::write(gate.with_extension("go"), b"go").unwrap();
        assert!(child.wait().unwrap().success());
        assert_eq!(clearer.load(&address()), None);
        assert!(!action_file(&root, &address().action_sha256, "json").exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn temporary_file_is_removed_after_write_or_sync_failure() {
        let (root, _) = roots("temporary-cleanup");
        fs::create_dir_all(&root).unwrap();
        let target = root.join("action.json");
        for after_sync in [false, true] {
            let result = publish_immutable_with(&target, b"payload", |file, bytes| {
                file.write_all(bytes)?;
                if after_sync {
                    file.sync_all()?;
                }
                Err(io::Error::other("simulated publication failure"))
            });
            assert_eq!(
                result.unwrap_err().to_string(),
                "simulated publication failure"
            );
            assert!(!target.exists());
            assert_eq!(fs::read_dir(&root).unwrap().count(), 0);
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn temporary_collision_does_not_remove_other_writers_file() {
        let (root, _) = roots("temporary-collision");
        fs::create_dir_all(&root).unwrap();
        let target = root.join("action.json");
        let temporary = root.join("action.tmp-existing");
        fs::write(&temporary, b"other writer").unwrap();
        let result = publish_immutable_with_temporary(&target, &temporary, b"payload", |_, _| {
            panic!("collision must not invoke writer")
        });
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read(&temporary).unwrap(), b"other writer");
        assert!(!target.exists());
        fs::remove_dir_all(root).unwrap();
    }

    /// Workers on one store, as processes (the CLI's default) or as threads
    /// of one process (the library's `thread` isolation, which shares a pid).
    fn workers_on_one_store(workers: usize, processes: bool) {
        let (root, project) = roots(&format!(
            "workers-{workers}-{}",
            if processes { "process" } else { "thread" }
        ));
        AuthenticatedOverlapCache::open(&root, &project).unwrap();
        let gate = root.join("workers");
        if processes {
            let mut children: Vec<_> = (0..workers)
                .map(|index| {
                    child("worker", &root, &project, &gate)
                        .env("GEOSPEC_CACHE_CHILD_NAME", index.to_string())
                        .env("GEOSPEC_CACHE_CHILD_WORKERS", workers.to_string())
                        .spawn()
                        .unwrap()
                })
                .collect();
            // Reap every worker, ending the rest as soon as one fails, so a
            // failed worker can neither hang the barrier nor leak a process.
            let deadline = Instant::now() + Duration::from_secs(120);
            let mut failed = false;
            let mut running: Vec<usize> = (0..workers).collect();
            while !running.is_empty() {
                running.retain(|&index| match children[index].try_wait().unwrap() {
                    Some(status) => {
                        failed |= !status.success();
                        false
                    }
                    None => true,
                });
                if failed || Instant::now() >= deadline {
                    for &index in &running {
                        let _ = children[index].kill();
                        let _ = children[index].wait();
                    }
                    break;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            assert!(
                !failed && running.is_empty(),
                "a worker failed or timed out"
            );
        } else {
            std::thread::scope(|scope| {
                for index in 0..workers {
                    let (root, project, gate) = (&root, &project, &gate);
                    scope.spawn(move || run_worker(root, project, gate, index, workers));
                }
            });
        }
        let cache = AuthenticatedOverlapCache::open(&root, &project).unwrap();
        let inventory = inventory_of(&cache);
        assert_eq!(inventory.actions.len(), 64 + workers * 64);
        // The stored rows are exactly the published ones, whoever won.
        let mut rows: Vec<_> = inventory
            .actions
            .iter()
            .map(|action| {
                (
                    action.record.envelope.action_sha256.clone(),
                    action.record.envelope.content_sha256.clone(),
                )
            })
            .collect();
        rows.sort();
        let mut expected: Vec<_> = (0..64)
            .map(|key| (fact_at(key), format!("shared-{key}")))
            .chain((0..workers).flat_map(|worker| {
                (0..64).map(move |key| {
                    (
                        fact_at(4096 + worker * 64 + key),
                        format!("worker-{worker}-{key}"),
                    )
                })
            }))
            .map(|(address, payload)| (address.action_sha256, sha256_hex(payload)))
            .collect();
        expected.sort();
        assert_eq!(rows, expected);
        for shard in fs::read_dir(managed(&root).join("facts")).unwrap() {
            assert!(temporaries_in(&shard.unwrap().path()).is_empty());
        }
        // Every worker returned its reservation: the ledger covers the store
        // and over-states by at most one chunk per worker.
        assert_covers(&root, &cache);
        let ledger = disk_ledger(&root);
        assert!(ledger.facts <= inventory.ledger().facts + workers as u64 * 128);
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn workers_as_processes_have_no_spurious_misses() {
        for workers in [1, 3, 11] {
            workers_on_one_store(workers, true);
        }
    }

    #[test]
    fn workers_as_threads_have_no_spurious_misses() {
        for workers in [1, 3, 11] {
            workers_on_one_store(workers, false);
        }
    }

    #[test]
    fn reads_hit_while_another_handle_holds_the_lock() {
        let (root, project) = roots("lock-free-read");
        let writer = AuthenticatedOverlapCache::open(&root, &project).unwrap();
        let reader = AuthenticatedOverlapCache::open(&root, &project).unwrap();
        writer.publish(&fact("aa"), b"fact");
        writer.publish(&address(), b"evidence");
        writer
            .with_lock_wait(PUBLISH_WAIT, || {
                assert_eq!(
                    reader.load(&fact("aa")).as_deref(),
                    Some(b"fact".as_slice())
                );
                assert_eq!(
                    reader.load(&address()).as_deref(),
                    Some(b"evidence".as_slice())
                );
                Ok(())
            })
            .unwrap();
        assert_eq!(reader.diagnostics.borrow().misses, 0);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn facts_family_has_its_own_cap_and_keeps_a_ledger() {
        let (root, project) = roots("facts-cap");
        let limits = Limits {
            bytes: 1024 * 1024,
            low_water: 512 * 1024,
            ..small_limits()
        };
        let cache = AuthenticatedOverlapCache::open_with_limits(&root, &project, limits).unwrap();
        for byte in ["11", "22", "33", "44"] {
            cache.publish(&addressed(byte), byte.as_bytes());
        }
        let facts: Vec<_> = (0..8).map(fact_at).collect();
        for (index, address) in facts.iter().enumerate() {
            cache.publish(address, format!("fact-{index}").as_bytes());
        }
        // Four evidence actions fill their cap without displacing a fact.
        for (index, address) in facts.iter().enumerate() {
            assert_eq!(
                cache.load(address),
                Some(format!("fact-{index}").into_bytes())
            );
        }
        cache.flush();
        let ledger = disk_ledger(&root);
        assert_eq!((ledger.evidence, ledger.facts), (4, 8));
        assert_eq!(counts(&ledger), counts(&inventory_of(&cache).ledger()));
        drop(cache);
        // A ninth fact evicts the oldest facts down to the low water, never
        // evidence.
        let cache = AuthenticatedOverlapCache::open_with_limits(&root, &project, limits).unwrap();
        cache.publish(&fact_at(8), b"fact-8");
        assert_eq!(
            cache.load(&fact_at(8)).as_deref(),
            Some(b"fact-8".as_slice())
        );
        cache.flush();
        let ledger = disk_ledger(&root);
        assert_eq!(counts(&ledger), counts(&inventory_of(&cache).ledger()));
        assert_eq!(ledger.evidence, 4);
        assert_eq!(ledger.facts, 8, "{ledger:?}");
        for byte in ["11", "22", "33", "44"] {
            assert_eq!(
                cache.load(&addressed(byte)).as_deref(),
                Some(byte.as_bytes())
            );
        }
        assert_eq!(cache.diagnostics.borrow().rejected_writes, 0);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn malformed_or_pending_ledger_is_recounted() {
        let (root, project) = roots("ledger-recount");
        let cache = AuthenticatedOverlapCache::open(&root, &project).unwrap();
        cache.publish(&addressed("aa"), b"one");
        cache.publish(&addressed("bb"), b"two");
        let ledger_path = managed(&root).join("ledger");
        assert_eq!(
            counts(&disk_ledger(&root)),
            counts(&inventory_of(&cache).ledger())
        );
        for damaged in [
            b"geospec-cache-ledger-v2 0 0 0 0 clean".as_slice(),
            b"geospec-cache-ledger-v2 0 0 1 7 pending\n",
            b"geospec-cache-ledger-v1 0 0 0 clean\n",
            b"garbage\n",
            b"",
        ] {
            let epoch = disk_ledger(&root).epoch;
            fs::write(&ledger_path, damaged).unwrap();
            let address = EvidenceAddress {
                action_sha256: format!("{:064x}", damaged.len() + 1),
                ..address()
            };
            cache.publish(&address, b"three");
            let recounted = disk_ledger(&root);
            assert_eq!(counts(&recounted), counts(&inventory_of(&cache).ledger()));
            assert_ne!(recounted.epoch, epoch, "a recount starts a new epoch");
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn divergent_fact_is_poisoned_like_evidence() {
        let (root, project) = roots("facts-poison");
        let cache = AuthenticatedOverlapCache::open(&root, &project).unwrap();
        cache.publish(&fact("aa"), b"first");
        cache.publish(&fact("aa"), b"first");
        assert_eq!(
            cache.load(&fact("aa")).as_deref(),
            Some(b"first".as_slice())
        );
        assert_eq!(cache.diagnostics.borrow().writes, 2);
        cache.publish(&fact("aa"), b"second");
        assert_eq!(cache.load(&fact("aa")), None);
        assert!(fact_file(&root, &fact("aa").action_sha256, "poison").exists());
        assert!(!fact_file(&root, &fact("aa").action_sha256, "fact").exists());
        // A poisoned action refuses every later publication.
        cache.publish(&fact("aa"), b"first");
        assert_eq!(cache.load(&fact("aa")), None);
        assert!(!fact_file(&root, &fact("aa").action_sha256, "fact").exists());
        cache.flush();
        assert_covers(&root, &cache);
        drop(cache);
        let reopened = AuthenticatedOverlapCache::open(&root, &project).unwrap();
        assert_eq!(reopened.load(&fact("aa")), None, "poison survives reopen");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn fact_is_one_record_linked_into_place() {
        let (root, project) = roots("fact-record");
        let cache = AuthenticatedOverlapCache::open(&root, &project).unwrap();
        let payload = br#"{"fact":"value"}"#;
        cache.publish(&fact("ab"), payload);
        let path = fact_file(&root, &fact("ab").action_sha256, "fact");
        let bytes = fs::read(&path).unwrap();
        let split = bytes.iter().position(|byte| *byte == b'\n').unwrap();
        assert_eq!(&bytes[split + 1..], payload);
        let header = &bytes[..split];
        assert_eq!(canonicalize(header).unwrap(), header);
        let record: ActionRecord = serde_json::from_slice(header).unwrap();
        assert_eq!(record.envelope.content_sha256, sha256_hex(payload));
        assert_eq!(record.envelope.content_byte_length, payload.len() as u64);
        // Nothing else: no content blob, no action file, no temporary.
        for directory in ["actions", "content"] {
            assert_eq!(
                fs::read_dir(managed(&root).join(directory))
                    .unwrap()
                    .count(),
                0
            );
        }
        let shard = path.parent().unwrap();
        assert_eq!(fs::read_dir(shard).unwrap().count(), 1);
        // A repeat is a no-op.
        cache.publish(&fact("ab"), payload);
        assert_eq!(fs::read_dir(shard).unwrap().count(), 1);
        assert_eq!(fs::read(&path).unwrap(), bytes);
        assert_eq!(cache.diagnostics.borrow().writes, 2);
        assert_eq!(cache.load(&fact("ab")).as_deref(), Some(payload.as_slice()));
        // Another profile at the same address misses.
        let other = EvidenceAddress {
            producer_profile_sha256: "33".repeat(32),
            ..fact("ab")
        };
        assert_eq!(cache.load(&other), None);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn managed_v3_store_is_left_in_place() {
        let (root, project) = roots("v3-left");
        let v3 = root.join("managed-v3");
        fs::create_dir_all(v3.join("actions").join("aa")).unwrap();
        fs::write(v3.join("ledger"), b"geospec-cache-ledger-v1 0 1 9 clean\n").unwrap();
        let old = v3
            .join("actions")
            .join("aa")
            .join(format!("{}.json", "aa".repeat(32)));
        fs::write(&old, b"v3 record").unwrap();
        let cache = AuthenticatedOverlapCache::open(&root, &project).unwrap();
        assert_eq!(cache.load(&fact("aa")), None);
        cache.publish(&fact("aa"), b"v4");
        assert!(cache.clear());
        assert_eq!(cache.load(&fact("aa")).as_deref(), Some(b"v4".as_slice()));
        assert_eq!(fs::read(&old).unwrap(), b"v3 record");
        assert_eq!(
            fs::read(v3.join("ledger")).unwrap(),
            b"geospec-cache-ledger-v1 0 1 9 clean\n"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn torn_fact_records_miss_and_are_replaced() {
        let (root, project) = roots("torn");
        let cache = AuthenticatedOverlapCache::open(&root, &project).unwrap();
        let payload = br#"{"fact":"torn"}"#;
        cache.publish(&fact("ab"), payload);
        let path = fact_file(&root, &fact("ab").action_sha256, "fact");
        let good = fs::read(&path).unwrap();
        let header = good.iter().position(|byte| *byte == b'\n').unwrap();
        // The same fact published under another installation key.
        let (foreign_root, _) = roots("torn-foreign");
        let foreign = AuthenticatedOverlapCache::open(&foreign_root, &project).unwrap();
        foreign.publish(&fact("ab"), payload);
        let foreign_record =
            fs::read(fact_file(&foreign_root, &fact("ab").action_sha256, "fact")).unwrap();
        drop(foreign);
        fs::remove_dir_all(foreign_root).unwrap();
        let flip = |offset: usize| {
            let mut bytes = good.clone();
            bytes[offset] ^= 0x01;
            bytes
        };
        let damages: Vec<(&str, Vec<u8>)> = vec![
            ("empty", Vec::new()),
            ("truncated in the header", good[..header / 2].to_vec()),
            ("truncated after the newline", good[..header + 1].to_vec()),
            ("truncated in the payload", good[..good.len() - 3].to_vec()),
            ("zero-filled", vec![0; good.len()]),
            ("payload bit flip", flip(good.len() - 2)),
            ("record bit flip", flip(header / 2)),
            ("another installation's record", foreign_record),
        ];
        for (name, damaged) in damages {
            fs::write(&path, &damaged).unwrap();
            assert_eq!(cache.load(&fact("ab")), None, "{name} misses");
            cache.publish(&fact("ab"), payload);
            assert_eq!(fs::read(&path).unwrap(), good, "{name} is replaced");
            assert_eq!(
                cache.load(&fact("ab")).as_deref(),
                Some(payload.as_slice()),
                "{name} hits once replaced"
            );
            assert!(!fact_file(&root, &fact("ab").action_sha256, "poison").exists());
        }
        let diagnostics = cache.diagnostics.borrow().clone();
        assert_eq!(
            (diagnostics.io_failures, diagnostics.rejected_writes),
            (0, 0)
        );
        assert!(temporaries_in(path.parent().unwrap()).is_empty());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_taken_name_is_replaced_only_after_a_complete_read_fails_integrity() {
        let (root, project) = roots("classify");
        let cache = AuthenticatedOverlapCache::open(&root, &project).unwrap();

        // A record that authenticates and differs only in its producer
        // profile is divergence: poison, never replacement.
        cache.publish(&fact("ac"), b"one");
        let profile = EvidenceAddress {
            producer_profile_sha256: "33".repeat(32),
            ..fact("ac")
        };
        cache.publish(&profile, b"one");
        assert!(fact_file(&root, &fact("ac").action_sha256, "poison").exists());
        assert_eq!(cache.load(&fact("ac")), None);
        assert_eq!(cache.load(&profile), None);

        // An operating-system error while reading aborts: nothing replaced,
        // nothing poisoned, one I/O failure.
        let path = fact_file(&root, &fact("ad").action_sha256, "fact");
        for damaged in [false, true] {
            let (root, project) = roots("classify-eio");
            let cache = AuthenticatedOverlapCache::open(&root, &project).unwrap();
            let path = fact_file(&root, &fact("ad").action_sha256, "fact");
            cache.publish(&fact("ad"), b"one");
            if damaged {
                fs::write(&path, b"torn").unwrap();
            }
            let before = fs::read(&path).unwrap();
            set_hook(|step, _| {
                if step == "existing" {
                    Err(io::Error::from_raw_os_error(libc::EIO))
                } else {
                    Ok(())
                }
            });
            cache.publish(&fact("ad"), b"two");
            clear_hook();
            assert_eq!(fs::read(&path).unwrap(), before);
            assert!(!path.with_extension("poison").exists());
            assert_eq!(cache.diagnostics.borrow().io_failures, 1);
            assert!(temporaries_in(path.parent().unwrap()).is_empty());
            fs::remove_dir_all(root).unwrap();
        }

        // A record that vanishes between the failed link and the read (an
        // eviction) is linked once more.
        cache.publish(&fact("ad"), b"one");
        let removed = Rc::new(Cell::new(false));
        let flag = removed.clone();
        set_hook(move |step, at| {
            if step == "existing" && !flag.get() {
                flag.set(true);
                fs::remove_file(at)?;
            }
            Ok(())
        });
        cache.publish(&fact("ad"), b"two");
        clear_hook();
        assert!(removed.get());
        assert_eq!(cache.load(&fact("ad")).as_deref(), Some(b"two".as_slice()));
        assert!(!path.with_extension("poison").exists());

        // Something that is not a plain file at the name aborts.
        let directory = fact_file(&root, &fact("ae").action_sha256, "fact");
        fs::create_dir_all(&directory).unwrap();
        let failures = cache.diagnostics.borrow().io_failures;
        cache.publish(&fact("ae"), b"one");
        assert!(directory.is_dir());
        assert!(!directory.with_extension("poison").exists());
        assert_eq!(cache.diagnostics.borrow().io_failures, failures + 1);
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn divergent_fact_processes_poison_in_either_order_and_concurrently() {
        for order in ["first-then-second", "second-then-first", "concurrent"] {
            let (root, project) = roots("fact-race");
            AuthenticatedOverlapCache::open(&root, &project).unwrap();
            let gate = root.join("start");
            let spawn = |name: &str| {
                child("fact-publish", &root, &project, &gate)
                    .env("GEOSPEC_CACHE_CHILD_NAME", name)
                    .spawn()
                    .unwrap()
            };
            if order == "concurrent" {
                let mut first = spawn("first");
                let mut second = spawn("second");
                wait_for(&gate.with_extension("first.ready"));
                wait_for(&gate.with_extension("second.ready"));
                fs::write(&gate, b"go").unwrap();
                assert!(first.wait().unwrap().success());
                assert!(second.wait().unwrap().success());
            } else {
                fs::write(&gate, b"go").unwrap();
                let names = if order == "first-then-second" {
                    ["first", "second"]
                } else {
                    ["second", "first"]
                };
                for name in names {
                    assert!(spawn(name).wait().unwrap().success());
                }
            }
            let cache = AuthenticatedOverlapCache::open(&root, &project).unwrap();
            assert_eq!(cache.load(&fact("ab")), None, "{order}");
            assert!(
                fact_file(&root, &fact("ab").action_sha256, "poison").exists(),
                "{order}"
            );
            fs::remove_dir_all(root).unwrap();
        }
    }

    #[cfg(unix)]
    #[test]
    fn killed_at_each_publish_step_never_serves_a_partial_record() {
        for step in ["created", "written", "linked", "unlinked"] {
            let (root, project) = roots(&format!("kill-{step}"));
            AuthenticatedOverlapCache::open(&root, &project).unwrap();
            let gate = root.join("paused");
            let mut writer = child("fact", &root, &project, &gate)
                .env("GEOSPEC_CACHE_PAUSE_AT", step)
                .env("GEOSPEC_CACHE_PAUSE_GATE", &gate)
                .spawn()
                .unwrap();
            wait_for(&gate);
            writer.kill().unwrap();
            writer.wait().unwrap();
            let record = fact_file(&root, &fact("ab").action_sha256, "fact");
            let shard = record.parent().unwrap().to_path_buf();
            let linked = matches!(step, "linked" | "unlinked");
            assert_eq!(
                temporaries_in(&shard).len(),
                usize::from(step != "unlinked")
            );
            // A young temporary may be a live writer's: recovery keeps it.
            fs::remove_file(managed(&root).join("ledger")).unwrap();
            let reopened = AuthenticatedOverlapCache::open(&root, &project).unwrap();
            assert_eq!(
                temporaries_in(&shard).len(),
                usize::from(step != "unlinked")
            );
            assert_eq!(
                reopened.load(&fact("ab")).as_deref(),
                linked.then_some(b"paused".as_slice()),
                "{step}"
            );
            // A republish succeeds and hits.
            reopened.publish(&fact("ab"), b"paused");
            assert_eq!(
                reopened.load(&fact("ab")).as_deref(),
                Some(b"paused".as_slice())
            );
            let diagnostics = reopened.diagnostics.borrow().clone();
            assert_eq!(
                (diagnostics.io_failures, diagnostics.rejected_writes),
                (0, 0)
            );
            drop(reopened);
            // Once old enough, recovery collects the leftover temporary; a
            // second link of the record leaves the record serving.
            fs::remove_file(managed(&root).join("ledger")).unwrap();
            let aged = Limits {
                temporary_age: Duration::ZERO,
                ..DEFAULT_LIMITS
            };
            let collected =
                AuthenticatedOverlapCache::open_with_limits(&root, &project, aged).unwrap();
            assert!(temporaries_in(&shard).is_empty(), "{step}");
            assert_eq!(
                collected.load(&fact("ab")).as_deref(),
                Some(b"paused".as_slice())
            );
            assert_eq!(
                counts(&disk_ledger(&root)),
                counts(&inventory_of(&collected).ledger())
            );
            fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    fn temporary_age_rule_holds_in_every_collector() {
        let (root, project) = roots("temporary-age");
        let limits = Limits {
            bytes: 1024 * 1024,
            low_water: 512 * 1024,
            ..small_limits()
        };
        let cache = AuthenticatedOverlapCache::open_with_limits(&root, &project, limits).unwrap();
        cache.publish(&fact("aa"), b"fact");
        let shard = managed(&root).join("facts").join("bb");
        fs::create_dir_all(&shard).unwrap();
        let digest = "bb".repeat(32);
        let young = shard.join(format!("{digest}.tmp-{}-0", "0".repeat(32)));
        let old = shard.join(format!("{digest}.tmp-{}-1", "0".repeat(32)));
        let make_old = || {
            fs::write(&old, b"abandoned").unwrap();
            File::options()
                .write(true)
                .open(&old)
                .unwrap()
                .set_modified(SystemTime::now() - Duration::from_secs(2 * 60 * 60))
                .unwrap();
        };
        fs::write(&young, b"in flight").unwrap();
        make_old();
        // Open-time recovery.
        fs::remove_file(managed(&root).join("ledger")).unwrap();
        drop(cache);
        let cache = AuthenticatedOverlapCache::open_with_limits(&root, &project, limits).unwrap();
        assert!(young.exists() && !old.exists(), "recovery");
        // Clear.
        make_old();
        assert!(cache.clear());
        assert!(young.exists(), "clear");
        // Eviction: more facts than the cap of 8.
        for index in 0..12 {
            cache.publish(&fact_at(index), b"fact");
        }
        assert!(test_support::EVICTIONS.with(Cell::get) > 0);
        assert!(young.exists() && !old.exists(), "eviction");
        assert_eq!(fs::read(&young).unwrap(), b"in flight");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn fact_temporary_collision_does_not_remove_other_writers_file() {
        let (root, project) = roots("fact-collision");
        let cache = AuthenticatedOverlapCache::open(&root, &project).unwrap();
        let other = AuthenticatedOverlapCache::open(&root, &project).unwrap();
        // Handles in one process name their temporaries apart.
        assert_ne!(cache.handle_id, other.handle_id);
        for id in [&cache.handle_id, &other.handle_id] {
            assert_eq!(id.len(), 32);
            assert!(id.bytes().all(|byte| byte.is_ascii_hexdigit()));
        }
        let digest = fact("ab").action_sha256;
        let shard = managed(&root).join("facts").join(&digest[..2]);
        fs::create_dir_all(&shard).unwrap();
        let taken = shard.join(format!(
            "{digest}.tmp-{}-{}",
            cache.handle_id,
            cache.temporaries.get()
        ));
        fs::write(&taken, b"other writer").unwrap();
        cache.publish(&fact("ab"), b"payload");
        assert_eq!(fs::read(&taken).unwrap(), b"other writer");
        assert!(!fact_file(&root, &digest, "fact").exists());
        assert_eq!(cache.diagnostics.borrow().io_failures, 1);
        // The next publication takes the next name.
        cache.publish(&fact("ab"), b"payload");
        assert_eq!(
            cache.load(&fact("ab")).as_deref(),
            Some(b"payload".as_slice())
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn failed_fact_publication_returns_its_reservation_unit() {
        let (root, project) = roots("failed-fact");
        let cache = AuthenticatedOverlapCache::open(&root, &project).unwrap();
        cache.publish(&fact("aa"), b"one");
        let before = cache.reservation.get();
        set_hook(|step, _| {
            if step == "written" {
                Err(io::Error::other("simulated write failure"))
            } else {
                Ok(())
            }
        });
        cache.publish(&fact("bb"), b"two");
        clear_hook();
        let after = cache.reservation.get();
        assert_eq!(
            (
                after.entries,
                after.bytes,
                after.used_entries,
                after.used_bytes
            ),
            (
                before.entries,
                before.bytes,
                before.used_entries,
                before.used_bytes
            )
        );
        assert_eq!(cache.diagnostics.borrow().io_failures, 1);
        let record = fact_file(&root, &fact("bb").action_sha256, "fact");
        assert!(!record.exists());
        assert!(temporaries_in(record.parent().unwrap()).is_empty());
        // A repeat gives its unit back too.
        cache.publish(&fact("aa"), b"one");
        assert_eq!(cache.reservation.get().entries, before.entries);
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn a_killed_writer_leaves_only_over_statement() {
        let (root, project) = roots("killed-reservation");
        AuthenticatedOverlapCache::open(&root, &project).unwrap();
        let gate = root.join("reserved");
        let mut writer = child("reserve", &root, &project, &gate).spawn().unwrap();
        wait_for(&gate);
        writer.kill().unwrap();
        writer.wait().unwrap();
        let cache = AuthenticatedOverlapCache::open(&root, &project).unwrap();
        assert_covers(&root, &cache);
        let ledger = disk_ledger(&root);
        let actual = inventory_of(&cache).ledger();
        assert_eq!(actual.facts, 10);
        // By at most the dead writer's one chunk.
        assert!(ledger.facts <= actual.facts + FIRST_CHUNK_ENTRIES);
        assert!(ledger.bytes <= actual.bytes + FIRST_CHUNK_BYTES);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_live_writer_restores_coverage_at_its_next_checkpoint() {
        let (root, project) = roots("recount-live");
        let writer = AuthenticatedOverlapCache::open(&root, &project).unwrap();
        for index in 0..5 {
            writer.publish(&fact_at(index), b"before");
        }
        // Another handle recounts: the writer's reservation is void.
        let other = AuthenticatedOverlapCache::open(&root, &project).unwrap();
        fs::remove_file(managed(&root).join("ledger")).unwrap();
        other.with_lock(|| other.ledger()).unwrap();
        assert_eq!(
            counts(&disk_ledger(&root)),
            counts(&inventory_of(&other).ledger())
        );
        // It keeps publishing from its remainder, uncounted, until its next
        // checkpoint re-adds what it published.
        let checkpoints = writer.checkpoints.get();
        let mut index = 5;
        while writer.checkpoints.get() == checkpoints {
            writer.publish(&fact_at(index), b"after");
            index += 1;
        }
        assert_covers(&root, &writer);
        writer.flush();
        assert_covers(&root, &writer);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn concurrent_writers_stay_within_the_soft_cap() {
        const WRITERS: usize = 4;
        let (root, project) = roots("soft-cap");
        let limits = Limits {
            fact_actions: 4096,
            bytes: 1024 * 1024 * 1024,
            low_water: 512 * 1024 * 1024,
            ..small_limits()
        };
        let chunk = limits.chunk_entries();
        assert_eq!(chunk, 8);
        let bound = limits.fact_actions as u64 + WRITERS as u64 * chunk;
        AuthenticatedOverlapCache::open_with_limits(&root, &project, limits).unwrap();
        let done = std::sync::atomic::AtomicUsize::new(0);
        let peak = std::thread::scope(|scope| {
            for writer in 0..WRITERS {
                let (root, project, done) = (&root, &project, &done);
                scope.spawn(move || {
                    let cache =
                        AuthenticatedOverlapCache::open_with_limits(root, project, limits).unwrap();
                    for index in 0..3_000 {
                        cache.publish(&fact_at(writer * 10_000 + index), b"fact");
                    }
                    let diagnostics = cache.diagnostics.borrow().clone();
                    assert_eq!(
                        (diagnostics.rejected_writes, diagnostics.io_failures),
                        (0, 0)
                    );
                    done.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                });
            }
            let observer =
                AuthenticatedOverlapCache::open_with_limits(&root, &project, limits).unwrap();
            let mut peak = 0;
            while done.load(std::sync::atomic::Ordering::SeqCst) < WRITERS {
                peak = peak.max(inventory_of(&observer).ledger().facts);
                std::thread::sleep(Duration::from_millis(50));
            }
            peak
        });
        let cache = AuthenticatedOverlapCache::open_with_limits(&root, &project, limits).unwrap();
        let actual = inventory_of(&cache).ledger().facts;
        eprintln!("soft cap: peak {peak}, final {actual}, bound {bound}");
        assert!(
            peak.max(actual) <= bound,
            "{peak} or {actual} above {bound}"
        );
        assert_covers(&root, &cache);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn one_eviction_pass_reaches_low_water_without_refusing() {
        let (root, project) = roots("evict-mixed");
        let limits = Limits {
            fact_actions: 4096,
            bytes: 4 * 1024 * 1024,
            low_water: 3 * 1024 * 1024,
            slice: 16,
            ..small_limits()
        };
        let cache = AuthenticatedOverlapCache::open_with_limits(&root, &project, limits).unwrap();
        for index in 0..2_000 {
            cache.publish(&fact_at(index), &[b's'; 400]);
        }
        let (inventories, evictions) = (
            test_support::INVENTORIES.with(Cell::get),
            test_support::EVICTIONS.with(Cell::get),
        );
        let large = vec![b'l'; 64 * 1024];
        let mut slowest = Duration::ZERO;
        for index in 0..64 {
            let start = Instant::now();
            cache.publish(&fact_at(100_000 + index), &large);
            slowest = slowest.max(start.elapsed());
            assert_eq!(
                cache.load(&fact_at(100_000 + index)).as_deref(),
                Some(large.as_slice())
            );
            assert!(disk_ledger(&root).bytes <= limits.bytes);
        }
        let inventories = test_support::INVENTORIES.with(Cell::get) - inventories;
        let evictions = test_support::EVICTIONS.with(Cell::get) - evictions;
        eprintln!("mixed sizes: {evictions} eviction(s), {inventories} inventories, slowest publication {slowest:?}");
        let diagnostics = cache.diagnostics.borrow().clone();
        assert_eq!(
            (diagnostics.rejected_writes, diagnostics.io_failures),
            (0, 0)
        );
        // About 6 MB into a 4 MiB store whose eviction frees 1 MiB a pass.
        assert!((1..=4).contains(&evictions), "{evictions} evictions");
        assert!(inventories <= 2 * evictions, "{inventories} inventories");
        // Oldest first: no small fact outlives a large one, and the newest
        // large facts, 2 MiB of them, stay.
        let small = (0..2_000).filter(|index| cache.load(&fact_at(*index)).is_some());
        let large: Vec<_> = (0..64)
            .map(|index| cache.load(&fact_at(100_000 + index)).is_some())
            .collect();
        if large.contains(&false) {
            assert_eq!(small.count(), 0);
        }
        assert!(large[32..].iter().all(|present| *present));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn fact_publication_syncs_only_at_checkpoints() {
        let (root, project) = roots("sync-count");
        let cache = AuthenticatedOverlapCache::open(&root, &project).unwrap();
        let syncs = test_support::SYNCS.with(Cell::get);
        let checkpoints = cache.checkpoints.get();
        for index in 0..1_000 {
            cache.publish(&fact_at(index), format!("fact-{index}").as_bytes());
        }
        let syncs = test_support::SYNCS.with(Cell::get) - syncs;
        let checkpoints = cache.checkpoints.get() - checkpoints;
        // 32, 64, then 128 entries a chunk: 2 + 8 checkpoints.
        assert_eq!(checkpoints, 10);
        // Each checkpoint syncs the ledger and its directory, nothing else.
        assert_eq!(syncs, 2 * checkpoints);
        let syncs = test_support::SYNCS.with(Cell::get);
        cache.flush();
        assert_eq!(
            test_support::SYNCS.with(Cell::get),
            syncs,
            "a return is not synced"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn drop_and_flush_do_not_wait_for_a_busy_lock() {
        let (root, project) = roots("drop-busy");
        let dropped = AuthenticatedOverlapCache::open(&root, &project).unwrap();
        let flushed = AuthenticatedOverlapCache::open(&root, &project).unwrap();
        dropped.publish(&fact("aa"), b"one");
        flushed.publish(&fact("bb"), b"two");
        let before = disk_ledger(&root);
        let (locked, release) = (std::sync::Barrier::new(2), std::sync::Barrier::new(2));
        std::thread::scope(|scope| {
            let (root, project, locked, release) = (&root, &project, &locked, &release);
            scope.spawn(move || {
                let holder = AuthenticatedOverlapCache::open(root, project).unwrap();
                holder
                    .with_lock_wait(PUBLISH_WAIT, || {
                        locked.wait();
                        release.wait();
                        Ok(())
                    })
                    .unwrap();
            });
            locked.wait();
            let start = Instant::now();
            drop(dropped);
            assert!(start.elapsed() < Duration::from_millis(100), "drop waited");
            let start = Instant::now();
            flushed.flush();
            assert!(start.elapsed() < Duration::from_millis(500), "flush waited");
            release.wait();
        });
        // Both returns were skipped: the ledger only over-states.
        assert_eq!(disk_ledger(&root), before);
        assert_covers(&root, &flushed);
        // A handle that never reserved never touches the ledger.
        let idle = AuthenticatedOverlapCache::open(&root, &project).unwrap();
        drop(idle);
        assert_eq!(disk_ledger(&root), before);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn eviction_publication_and_loads_race_without_a_false_hit() {
        let (root, project) = roots("stress");
        let limits = Limits {
            actions: 4,
            fact_actions: 64,
            bytes: 16 * 1024 * 1024,
            low_water: 8 * 1024 * 1024,
            slice: 4,
            temporary_age: TEMPORARY_AGE,
        };
        let payload = |key: usize| format!("value-{key}-").repeat(1 + key % 7).into_bytes();
        let setup = AuthenticatedOverlapCache::open_with_limits(&root, &project, limits).unwrap();
        let poisoned: Vec<_> = ["p1", "p2", "p3"]
            .iter()
            .map(|name| EvidenceAddress {
                action_sha256: sha256_hex(name),
                ..fact("00")
            })
            .collect();
        for address in &poisoned {
            setup.publish(address, b"one");
            setup.publish(address, b"two");
            assert!(fact_file(&root, &address.action_sha256, "poison").exists());
        }
        drop(setup);
        let poisoned = &poisoned;
        let evictions = std::sync::atomic::AtomicU64::new(0);
        std::thread::scope(|scope| {
            for writer in 0..3 {
                let (root, project, evictions) = (&root, &project, &evictions);
                scope.spawn(move || {
                    let cache =
                        AuthenticatedOverlapCache::open_with_limits(root, project, limits).unwrap();
                    for round in 0..400 {
                        let key = (round * 7 + writer * 13) % 300;
                        cache.publish(&fact_at(key), &payload(key));
                    }
                    for address in poisoned {
                        cache.publish(address, b"one");
                    }
                    let diagnostics = cache.diagnostics.borrow().clone();
                    assert_eq!(diagnostics.rejected_writes, 0);
                    // Only the three refusals at poisoned actions.
                    assert_eq!(diagnostics.io_failures, 3);
                    cache.flush();
                    evictions.fetch_add(
                        test_support::EVICTIONS.with(Cell::get),
                        std::sync::atomic::Ordering::SeqCst,
                    );
                });
            }
            for reader in 0..2 {
                let (root, project) = (&root, &project);
                scope.spawn(move || {
                    let cache =
                        AuthenticatedOverlapCache::open_with_limits(root, project, limits).unwrap();
                    for round in 0..3_000 {
                        let key = (round * 11 + reader) % 300;
                        if let Some(bytes) = cache.load(&fact_at(key)) {
                            assert_eq!(bytes, payload(key), "false hit for {key}");
                        }
                        assert_eq!(cache.load(&poisoned[round % 3]), None);
                    }
                });
            }
        });
        assert!(evictions.into_inner() > 0, "the stress never evicted");
        let cache = AuthenticatedOverlapCache::open_with_limits(&root, &project, limits).unwrap();
        for address in poisoned {
            assert!(fact_file(&root, &address.action_sha256, "poison").exists());
            assert_eq!(cache.load(address), None);
        }
        for key in 0..300 {
            if let Some(bytes) = cache.load(&fact_at(key)) {
                assert_eq!(bytes, payload(key));
            }
        }
        assert_covers(&root, &cache);
        fs::remove_dir_all(root).unwrap();
    }
}
