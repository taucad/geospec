//! Authenticated filesystem storage for native completed evidence families.
//!
//! The store is content-addressed and write-once per action. Its HMAC key
//! lives in the store root, so the envelope binds entries to one installation
//! and detects corruption and divergence; it is not a boundary against a local
//! writer who can read the directory.

use std::{
    cell::{Cell, RefCell},
    collections::HashSet,
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    time::{Duration, Instant},
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
/// v3 shards `actions/` and `content/` by the first two digest characters and
/// keeps a count and byte ledger. v2 directories are left untouched and miss.
const MANAGED_DIRECTORY: &str = "managed-v3";
const LEDGER_SCHEMA: &str = "geospec-cache-ledger-v1";
const MAX_EVIDENCE_ACTIONS: usize = 256;
/// One small action per (subject, scope, fact): tens of thousands of subjects.
const MAX_FACT_ACTIONS: usize = 65_536;
const MAX_STORE_BYTES: u64 = 512 * 1024 * 1024;
const LOW_WATER_BYTES: u64 = 384 * 1024 * 1024;
const COLLECTION_SLICE: usize = 16;
/// Publication waits this long for a writer; a read never takes the lock.
const PUBLISH_WAIT: Duration = Duration::from_secs(1);
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
}

impl Limits {
    fn cap(&self, class: Class) -> usize {
        match class {
            Class::Evidence => self.actions,
            Class::Facts => self.fact_actions,
        }
    }

    /// Eviction for a class over its count stops at this many actions.
    fn low_water_actions(&self, class: Class) -> usize {
        match class {
            Class::Evidence => self.actions,
            Class::Facts => self.fact_actions - self.fact_actions / 8,
        }
    }

    /// A hard bound on directory entries an inventory reads before refusing.
    fn inventory_bound(&self) -> usize {
        2 * (self.actions + self.fact_actions) + 4 * self.slice + 2
    }
}

const DEFAULT_LIMITS: Limits = Limits {
    actions: MAX_EVIDENCE_ACTIONS,
    fact_actions: MAX_FACT_ACTIONS,
    bytes: MAX_STORE_BYTES,
    low_water: LOW_WATER_BYTES,
    slice: COLLECTION_SLICE,
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
    modified: std::time::SystemTime,
    bytes: u64,
    poisoned: bool,
}

struct IndexedContent {
    path: PathBuf,
    digest: Option<String>,
}

struct Inventory {
    actions: Vec<IndexedAction>,
    content: Vec<IndexedContent>,
    temporary_actions: Vec<PathBuf>,
    /// Every file in the action shards, poison markers and temporaries included.
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
            evidence: count(Class::Evidence),
            facts: count(Class::Facts),
            bytes: self.bytes,
            pending: false,
        }
    }
}

/// Action counts and stored bytes, maintained under the lock so publication
/// never scans the store. `pending` marks a publication in progress; a
/// missing, malformed or pending ledger makes the next locked operation
/// recount from a full inventory. Counts may only over-state the store.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Ledger {
    evidence: u64,
    facts: u64,
    bytes: u64,
    pending: bool,
}

impl Ledger {
    fn count(&self, class: Class) -> u64 {
        match class {
            Class::Evidence => self.evidence,
            Class::Facts => self.facts,
        }
    }

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
            "{LEDGER_SCHEMA} {} {} {} {}\n",
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
        let (evidence, facts, total) = (number()?, number()?, number()?);
        let pending = match fields.next()? {
            "pending" => true,
            "clean" => false,
            _ => return None,
        };
        let ledger = Self {
            evidence,
            facts,
            bytes: total,
            pending,
        };
        (fields.next().is_none() && ledger.encode() == bytes).then_some(ledger)
    }
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
    flush_result: RefCell<Option<Vec<u8>>>,
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
        fs::create_dir_all(root.as_ref())?;
        let root = fs::canonicalize(root)?;
        let project = fs::canonicalize(project_root)?;
        if root.starts_with(&project) || project.starts_with(&root) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "GeoSpec cache root must not overlap the project tree.",
            ));
        }
        let secret = load_or_create_secret(&root)?;
        // Older binaries use root/actions, root/content or managed-v2. They
        // remain untouched; this managed generation safely misses them.
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
        let lock_path = root.join("cache.lock");
        let mut options = OpenOptions::new();
        options.write(true).create(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
        }
        options.open(&lock_path)?;
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
            flush_result: RefCell::new(None),
        };
        // Recover from a missing or pending ledger: a bounded number of
        // blob-before-action crash leftovers is reclaimed and the ledger is
        // recounted. A clean ledger costs one small read. A failed inventory
        // never deletes ambiguous data.
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

    /// Seals publication and returns the same canonical diagnostics on every
    /// later call.
    pub fn flush(&self) -> Vec<u8> {
        if let Some(result) = self.flush_result.borrow().as_ref() {
            return result.clone();
        }
        self.sealed.set(true);
        self.diagnostics.borrow_mut().sealed = true;
        let bytes = canonicalize(
            &serde_json::to_vec(&*self.diagnostics.borrow()).expect("diagnostics serialize"),
        )
        .expect("diagnostics canonicalize");
        *self.flush_result.borrow_mut() = Some(bytes.clone());
        bytes
    }

    /// Removes authenticated B48 actions in this managed generation. Content
    /// shared with other actions and poison markers remain; a crash before
    /// completion may leave B48 actions, but never acknowledges the clear.
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
            self.write_ledger(Ledger {
                pending: true,
                ..inventory.ledger()
            })?;
            for action in &inventory.actions {
                if action.record.envelope.family == OVERLAP_FAMILY && !action.poisoned {
                    fs::remove_file(&action.path)?;
                    sync_parent(&action.path)?;
                }
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

impl OverlapEvidenceCache for AuthenticatedOverlapCache {
    /// Lock-free: actions and content are immutable files published by
    /// atomic link, and the poison marker is checked before and after.
    fn load(&self, address: &EvidenceAddress) -> Option<Vec<u8>> {
        self.diagnostics.borrow_mut().reads += 1;
        let result = self.load_inner(address);
        let mut diagnostics = self.diagnostics.borrow_mut();
        if result.is_some() {
            diagnostics.hits += 1;
        } else {
            diagnostics.misses += 1;
        }
        result
    }

    fn publish(&self, address: &EvidenceAddress, payload: &[u8]) {
        if self.sealed.get() || payload.len() > MAX_PAYLOAD_BYTES {
            self.diagnostics.borrow_mut().rejected_writes += 1;
            return;
        }
        match self.with_lock_wait(PUBLISH_WAIT, || self.publish_inner(address, payload)) {
            Ok(()) => self.diagnostics.borrow_mut().writes += 1,
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
            File::open(&self.root)?.sync_all()?;
        }
        let path = self.generation_path();
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut file) => {
                file.write_all(b"0\n")?;
                file.sync_all()?;
                File::open(&self.root)?.sync_all()?;
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
            file.sync_all()?;
            fs::rename(&temporary, &path)?;
            File::open(&self.root)?.sync_all()
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result
    }

    /// The ledger, recounted from a full inventory when it is missing,
    /// malformed or left pending by an interrupted publication. Lock held.
    fn ledger(&self) -> io::Result<Ledger> {
        let path = self.ledger_path();
        match read_bounded(&path, 128) {
            Ok(bytes) => match Ledger::decode(&bytes) {
                Some(ledger) if !ledger.pending => return Ok(ledger),
                _ => {}
            },
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        self.collect_orphans(self.limits.slice)?;
        self.recount()
    }

    /// Rewrites the ledger from a full inventory. Lock held.
    fn recount(&self) -> io::Result<Ledger> {
        let ledger = self.inventory()?.ledger();
        self.write_ledger(ledger)?;
        Ok(ledger)
    }

    /// Replaces the ledger by rename. It is not synced: a ledger lost or
    /// truncated by a crash fails to decode and is recounted. Lock held.
    fn write_ledger(&self, ledger: Ledger) -> io::Result<()> {
        let temporary = self.root.join("ledger.tmp");
        let mut options = OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NOFOLLOW);
        }
        options.open(&temporary)?.write_all(&ledger.encode())?;
        fs::rename(&temporary, self.ledger_path())
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
            || class.is_none()
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "unknown or invalid cache action",
            ));
        }
        let envelope_bytes = canonicalize(&serde_json::to_vec(envelope).map_err(io::Error::other)?)
            .map_err(io::Error::other)?;
        let mut mac = HmacSha256::new_from_slice(&self.secret)
            .map_err(|_| io::Error::other("invalid HMAC key"))?;
        mac.update(&envelope_bytes);
        let supplied = decode_hex_32(&record.hmac_sha256).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "invalid cache action HMAC")
        })?;
        mac.verify_slice(&supplied).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "cache action authentication failed",
            )
        })?;
        Ok((record, class.expect("checked above")))
    }

    /// The files of every two-character shard of `directory`, each checked to
    /// carry a digest stem in its own shard; anything else refuses.
    fn shard_files(&self, directory: &str) -> io::Result<Vec<(PathBuf, fs::Metadata, String)>> {
        let mut files = Vec::new();
        for shard in fs::read_dir(self.root.join(directory))? {
            let shard = shard?;
            let shard_path = shard.path();
            let shard_name = shard.file_name();
            let shard_name = shard_name.to_str().unwrap_or_default();
            if shard_name.len() != 2
                || !shard_name
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
                || !fs::symlink_metadata(&shard_path)?.file_type().is_dir()
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "unknown cache shard entry",
                ));
            }
            for entry in fs::read_dir(&shard_path)? {
                let entry = entry?;
                if files.len() >= self.limits.inventory_bound() {
                    return Err(io::Error::new(
                        io::ErrorKind::StorageFull,
                        "cache inventory exceeds bound",
                    ));
                }
                let path = entry.path();
                let metadata = fs::symlink_metadata(&path)?;
                let digest = path
                    .file_stem()
                    .and_then(|stem| stem.to_str())
                    .unwrap_or_default()
                    .to_owned();
                if !metadata.file_type().is_file()
                    || !valid_digest(&digest)
                    || &digest[..2] != shard_name
                {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "unknown cache entry",
                    ));
                }
                files.push((path, metadata, digest));
            }
        }
        Ok(files)
    }

    /// A hard-bounded inventory, used for recovery, eviction and clear only.
    /// Unknown entries refuse collection rather than guessing reachability.
    fn inventory(&self) -> io::Result<Inventory> {
        let mut actions = Vec::new();
        let mut temporary_actions = Vec::new();
        let mut poison = HashSet::new();
        let mut bytes = 0_u64;
        let files = self.shard_files("actions")?;
        #[cfg(test)]
        let entries = files.len();
        for (path, metadata, digest) in files {
            if metadata.len() > MAX_ACTION_BYTES as u64 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "unknown cache action entry",
                ));
            }
            bytes = bytes.saturating_add(metadata.len());
            match path.extension().and_then(|extension| extension.to_str()) {
                Some("poison") => {
                    poison.insert(digest);
                }
                Some("json") => {
                    let (record, class) = self.read_action(&path)?;
                    actions.push(IndexedAction {
                        record,
                        class,
                        modified: metadata.modified()?,
                        path,
                        bytes: metadata.len(),
                        poisoned: false,
                    });
                }
                Some(extension) if extension.starts_with("tmp-") => temporary_actions.push(path),
                _ => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "unknown cache action entry",
                    ))
                }
            }
        }
        for action in &mut actions {
            action.poisoned = poison.contains(&action.record.envelope.action_sha256);
        }
        let mut content = Vec::new();
        for (path, metadata, stem) in self.shard_files("content")? {
            if metadata.len() > MAX_PAYLOAD_BYTES as u64 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "unknown cache content entry",
                ));
            }
            let extension = path
                .extension()
                .and_then(|value| value.to_str())
                .unwrap_or_default();
            let digest = if extension == "json" {
                Some(stem)
            } else if extension.starts_with("tmp-") {
                None
            } else {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "unknown cache content entry",
                ));
            };
            bytes = bytes.saturating_add(metadata.len());
            content.push(IndexedContent { path, digest });
        }
        let present: HashSet<_> = content
            .iter()
            .filter_map(|blob| blob.digest.as_deref())
            .collect();
        if actions
            .iter()
            .any(|action| !present.contains(action.record.envelope.content_sha256.as_str()))
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "cache action lacks content",
            ));
        }
        Ok(Inventory {
            actions,
            content,
            temporary_actions,
            #[cfg(test)]
            entries,
            bytes,
        })
    }

    /// Removes at most `quota` temporaries and unreferenced blobs. Lock held.
    fn collect_orphans(&self, quota: usize) -> io::Result<()> {
        let inventory = self.inventory()?;
        let referenced: HashSet<_> = inventory
            .actions
            .iter()
            .map(|action| action.record.envelope.content_sha256.as_str())
            .collect();
        let mut removed = 0;
        for path in &inventory.temporary_actions {
            if removed >= quota {
                break;
            }
            fs::remove_file(path)?;
            sync_parent(path)?;
            removed += 1;
        }
        for blob in &inventory.content {
            if removed >= quota {
                break;
            }
            if blob
                .digest
                .as_deref()
                .is_none_or(|digest| !referenced.contains(digest))
            {
                fs::remove_file(&blob.path)?;
                sync_parent(&blob.path)?;
                removed += 1;
            }
        }
        Ok(())
    }

    /// Makes room for one more action of `class` and `added` bytes, from the
    /// ledger alone unless eviction is needed. Lock held.
    fn ensure_capacity(&self, class: Class, action_sha: &str, added: u64) -> io::Result<Ledger> {
        let ledger = self.ledger()?;
        let fits = |ledger: &Ledger| {
            ledger.bytes.saturating_add(added) <= self.limits.bytes
                && ledger.count(class) < self.limits.cap(class) as u64
        };
        if fits(&ledger) {
            return Ok(ledger);
        }
        self.collect_orphans(self.limits.slice)?;
        let mut inventory = self.inventory()?;
        inventory.actions.sort_by_key(|action| action.modified);
        let over_count = ledger.count(class) >= self.limits.cap(class) as u64;
        let mut current = inventory.ledger();
        let quota = if over_count {
            (current.count(class) as usize)
                .saturating_sub(self.limits.low_water_actions(class))
                .max(self.limits.slice)
        } else {
            self.limits.slice
        };
        let mut removed = 0;
        for action in &inventory.actions {
            let done = if over_count {
                current.count(class) <= self.limits.low_water_actions(class) as u64
                    && fits(&current)
            } else {
                current.bytes <= self.limits.low_water && fits(&current)
            };
            if removed >= quota || done {
                break;
            }
            if action.poisoned
                || action.record.envelope.action_sha256 == action_sha
                || (over_count && action.class != class)
            {
                continue;
            }
            fs::remove_file(&action.path)?;
            sync_parent(&action.path)?;
            current = current.with(action.class, -1, -(action.bytes as i64));
            removed += 1;
        }
        if removed > 0 {
            self.collect_orphans(removed + self.limits.slice)?;
        }
        let ledger = self.recount()?;
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
        let envelope_bytes = canonicalize(&serde_json::to_vec(&record.envelope).ok()?).ok()?;
        let mut mac = HmacSha256::new_from_slice(&self.secret).ok()?;
        mac.update(&envelope_bytes);
        let supplied = decode_hex_32(&record.hmac_sha256)?;
        if mac.verify_slice(&supplied).is_err() {
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

    /// Poisons a divergent action and removes it. Lock held.
    fn poison(&self, action: &Path, class: Class) -> io::Result<()> {
        let ledger = self.ledger()?;
        let poison = action.with_extension("poison");
        publish_immutable(&poison, b"divergent action\n")?;
        let marker = b"divergent action\n".len() as i64;
        let ledger = match fs::symlink_metadata(action) {
            Ok(metadata) => {
                fs::remove_file(action)?;
                sync_parent(action)?;
                ledger.with(class, -1, marker - metadata.len() as i64)
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => ledger.with(class, 0, marker),
            Err(error) => return Err(error),
        };
        self.write_ledger(ledger)
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
        let content_sha256 = sha256_hex(payload);
        let content = self.content_path(&content_sha256);
        let envelope = Envelope {
            schema: ENVELOPE_SCHEMA.into(),
            authority_id: self.authority_id.clone(),
            key_id: self.key_id.clone(),
            action_sha256: address.action_sha256.clone(),
            family: address.family.into(),
            codec: address.codec.into(),
            producer_profile_sha256: address.producer_profile_sha256.clone(),
            content_sha256: content_sha256.clone(),
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
        let action = self.action_path(&address.action_sha256, "json");
        if action.with_extension("poison").exists() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "poisoned evidence action",
            ));
        }
        if action.exists() {
            self.read_action(&action)?;
            let existing = read_bounded(&action, MAX_ACTION_BYTES)?;
            let existing_content = read_bounded(&content, MAX_PAYLOAD_BYTES);
            if existing == bytes && existing_content.is_ok_and(|stored| stored == payload) {
                return Ok(());
            }
            self.poison(&action, class)?;
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "divergent evidence action",
            ));
        }
        let new_content = !content.exists();
        let added = bytes.len() as u64 + if new_content { payload.len() as u64 } else { 0 };
        let ledger = self.ensure_capacity(class, &address.action_sha256, added)?;
        let next = ledger.with(class, 1, added as i64);
        self.write_ledger(Ledger {
            pending: true,
            ..next
        })?;
        ensure_plain_directory(content.parent().expect("content shard"))?;
        ensure_plain_directory(action.parent().expect("action shard"))?;
        publish_immutable(&content, payload)?;
        if let Err(error) = publish_immutable(&action, &bytes) {
            if error.kind() == io::ErrorKind::AlreadyExists {
                self.poison(&action, class)?;
            }
            return Err(error);
        }
        self.write_ledger(next)
    }
}

fn sync_parent(path: &Path) -> io::Result<()> {
    File::open(path.parent().expect("cache entry has a parent"))?.sync_all()
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
    file.sync_all()?;
    match fs::hard_link(&temporary, &key) {
        Ok(()) => File::open(&authority)?.sync_all()?,
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
        file.sync_all()
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
        File::open(path.parent().expect("published file has a parent"))?.sync_all()
    })();
    if created {
        let _ = fs::remove_file(temporary);
    }
    result
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
    fn clear_refuses_ambiguous_action_before_removing_any_family() {
        let (root, project) = roots("ambiguous-clear");
        let cache = AuthenticatedOverlapCache::open(&root, &project).unwrap();
        let overlap = address();
        let mut exact = address();
        exact.action_sha256 = "44".repeat(32);
        exact.family = "native-exact-step-clusters-v1";
        exact.codec = "geospec-exact-clusters-json-v1";
        cache.publish(&overlap, b"overlap");
        cache.publish(&exact, b"exact");
        fs::write(action_file(&root, &exact.action_sha256, "json"), b"invalid").unwrap();
        assert!(!cache.clear());
        assert_eq!(cache.load(&overlap).as_deref(), Some(b"overlap".as_slice()));
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
    #[ignore = "manual facts-family publish cost diagnostic; writes 10,000 small files"]
    fn facts_publish_cost_is_flat_in_store_size_diagnostic() {
        let (root, project) = roots("facts-cost");
        let cache = AuthenticatedOverlapCache::open(&root, &project).unwrap();
        let mut marks = Vec::new();
        let start = Instant::now();
        for index in 0..5_000 {
            cache.publish(&fact_at(index), format!("fact-{index}").as_bytes());
            if index % 1_000 == 999 {
                marks.push(start.elapsed());
            }
        }
        let start = Instant::now();
        for index in 0..5_000 {
            assert!(cache.load(&fact_at(index)).is_some());
        }
        let hits = start.elapsed();
        let start = Instant::now();
        drop(cache);
        let reopened = AuthenticatedOverlapCache::open(&root, &project).unwrap();
        let reopen = start.elapsed();
        assert_eq!(reopened.diagnostics.borrow().rejected_writes, 0);
        eprintln!(
            "facts publish cumulative per 1000={marks:?} hits(5000)={hits:?} reopen={reopen:?}"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn unknown_inventory_refuses_clear_and_recovery_without_deletion() {
        let (root, project) = roots("unknown");
        let cache = AuthenticatedOverlapCache::open(&root, &project).unwrap();
        cache.publish(&address(), b"known");
        let unknown = managed(&root).join("actions").join("unknown.dat");
        fs::write(&unknown, b"foreign").unwrap();
        // Publication within capacity reads only the ledger; clear, eviction
        // and recovery read the inventory and refuse without deleting.
        cache.publish(&addressed("22"), b"new");
        assert_eq!(
            cache.load(&addressed("22")).as_deref(),
            Some(b"new".as_slice())
        );
        assert!(!cache.clear());
        fs::remove_file(managed(&root).join("ledger")).unwrap();
        drop(cache);
        assert!(AuthenticatedOverlapCache::open(&root, &project).is_err());
        assert!(action_file(&root, &address().action_sha256, "json").exists());
        assert_eq!(fs::read(unknown).unwrap(), b"foreign");
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

    #[test]
    fn cache_process_child() {
        let Ok(mode) = std::env::var("GEOSPEC_CACHE_CHILD_MODE") else {
            return;
        };
        let root = PathBuf::from(std::env::var("GEOSPEC_CACHE_CHILD_ROOT").unwrap());
        let project = PathBuf::from(std::env::var("GEOSPEC_CACHE_CHILD_PROJECT").unwrap());
        let gate = PathBuf::from(std::env::var("GEOSPEC_CACHE_CHILD_GATE").unwrap());
        let cache = AuthenticatedOverlapCache::open(&root, &project).unwrap();
        if mode == "publish" {
            let name = std::env::var("GEOSPEC_CACHE_CHILD_NAME").unwrap();
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
        } else if mode == "stale" {
            assert_eq!(cache.load(&address()), None);
            fs::write(&gate, b"missed").unwrap();
            while !gate.with_extension("go").exists() {
                std::thread::sleep(Duration::from_millis(1));
            }
            cache.publish(&address(), b"stale computation");
        } else if mode == "worker" {
            // One CI worker: publish every shared fact and its own, wait for
            // the others, then read every fact any worker published.
            let index: usize = std::env::var("GEOSPEC_CACHE_CHILD_NAME")
                .unwrap()
                .parse()
                .unwrap();
            let workers: usize = std::env::var("GEOSPEC_CACHE_CHILD_WORKERS")
                .unwrap()
                .parse()
                .unwrap();
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
            fs::write(gate.with_extension(format!("{index}.ready")), b"ready").unwrap();
            while (0..workers).any(|other| !gate.with_extension(format!("{other}.ready")).exists())
            {
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
        } else if mode == "hold" || mode == "orphan" {
            cache
                .with_lock_wait(PUBLISH_WAIT, || {
                    if mode == "orphan" {
                        // The publication order: pending ledger, then blob.
                        let bytes = b"orphan blob";
                        let ledger = cache.ledger()?;
                        cache.write_ledger(Ledger {
                            pending: true,
                            ..ledger
                        })?;
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

    #[cfg(unix)]
    #[test]
    fn n_workers_on_one_store_have_no_spurious_misses() {
        const WORKERS: usize = 4;
        let (root, project) = roots("workers");
        AuthenticatedOverlapCache::open(&root, &project).unwrap();
        let gate = root.join("workers");
        let children: Vec<_> = (0..WORKERS)
            .map(|index| {
                Command::new(std::env::current_exe().unwrap())
                    .arg("--exact")
                    .arg("tests::cache_process_child")
                    .env("GEOSPEC_CACHE_CHILD_MODE", "worker")
                    .env("GEOSPEC_CACHE_CHILD_ROOT", &root)
                    .env("GEOSPEC_CACHE_CHILD_PROJECT", &project)
                    .env("GEOSPEC_CACHE_CHILD_GATE", &gate)
                    .env("GEOSPEC_CACHE_CHILD_NAME", index.to_string())
                    .env("GEOSPEC_CACHE_CHILD_WORKERS", WORKERS.to_string())
                    .spawn()
                    .unwrap()
            })
            .collect();
        for mut child in children {
            assert!(child.wait().unwrap().success());
        }
        let cache = AuthenticatedOverlapCache::open(&root, &project).unwrap();
        let inventory = cache
            .with_lock_wait(PUBLISH_WAIT, || cache.inventory())
            .unwrap();
        assert_eq!(inventory.actions.len(), 64 + WORKERS * 64);
        assert_eq!(
            cache
                .with_lock_wait(PUBLISH_WAIT, || cache.ledger())
                .unwrap(),
            inventory.ledger()
        );
        fs::remove_dir_all(root).unwrap();
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
        let ledger = cache
            .with_lock_wait(PUBLISH_WAIT, || cache.ledger())
            .unwrap();
        assert_eq!((ledger.evidence, ledger.facts), (4, 8));
        assert_eq!(
            ledger,
            cache
                .with_lock_wait(PUBLISH_WAIT, || cache.inventory())
                .unwrap()
                .ledger()
        );
        // A ninth fact evicts the oldest facts down to the low water, never evidence.
        cache.publish(&fact_at(8), b"fact-8");
        assert_eq!(
            cache.load(&fact_at(8)).as_deref(),
            Some(b"fact-8".as_slice())
        );
        let ledger = cache
            .with_lock_wait(PUBLISH_WAIT, || cache.ledger())
            .unwrap();
        assert_eq!(
            ledger,
            cache
                .with_lock_wait(PUBLISH_WAIT, || cache.inventory())
                .unwrap()
                .ledger()
        );
        assert_eq!(ledger.evidence, 4);
        assert!(ledger.facts <= 8 && ledger.facts >= 2, "{ledger:?}");
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
        cache.publish(&fact("aa"), b"one");
        cache.publish(&fact("bb"), b"two");
        let ledger_path = managed(&root).join("ledger");
        let expected = cache
            .with_lock_wait(PUBLISH_WAIT, || cache.inventory())
            .unwrap()
            .ledger();
        assert_eq!(
            Ledger::decode(&fs::read(&ledger_path).unwrap()),
            Some(expected)
        );
        for damaged in [
            b"geospec-cache-ledger-v1 0 0 0 clean".as_slice(),
            b"geospec-cache-ledger-v1 0 1 7 pending\n",
            b"garbage\n",
            b"",
        ] {
            fs::write(&ledger_path, damaged).unwrap();
            cache.publish(&fact_at(damaged.len()), b"three");
            let recounted = cache
                .with_lock_wait(PUBLISH_WAIT, || cache.inventory())
                .unwrap()
                .ledger();
            assert_eq!(
                Ledger::decode(&fs::read(&ledger_path).unwrap()),
                Some(recounted)
            );
        }
        assert_eq!(
            cache.load(&fact_at(0)).as_deref(),
            Some(b"three".as_slice())
        );
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
        cache.publish(&fact("aa"), b"second");
        assert_eq!(cache.load(&fact("aa")), None);
        assert!(action_file(&root, &fact("aa").action_sha256, "poison").exists());
        let ledger = cache
            .with_lock_wait(PUBLISH_WAIT, || cache.ledger())
            .unwrap();
        assert_eq!(
            ledger,
            cache
                .with_lock_wait(PUBLISH_WAIT, || cache.inventory())
                .unwrap()
                .ledger()
        );
        fs::remove_dir_all(root).unwrap();
    }
}
