//! Synchronous wasm32 C ABI for the configured GeoSpec native engine.

use std::{cell::RefCell, collections::BTreeMap, mem::MaybeUninit};

use geospec_engine_native_core::canonicalize as core_canonicalize;
use geospec_engine_native_runtime::{create_engine, Engine, EngineConfig, ProtocolError};

#[derive(Debug)]
struct Failure {
    code: Vec<u8>,
    message: Vec<u8>,
}

impl From<ProtocolError> for Failure {
    fn from(error: ProtocolError) -> Self {
        Self {
            code: error.code().as_bytes().to_vec(),
            message: error.to_string().into_bytes(),
        }
    }
}

#[derive(Debug)]
struct AbiResult(Result<Vec<u8>, Failure>);

#[derive(Debug)]
struct Arena<T> {
    next: u32,
    entries: BTreeMap<u32, T>,
}

impl<T> Arena<T> {
    fn new() -> Self {
        Self {
            next: 1,
            entries: BTreeMap::new(),
        }
    }

    fn insert(&mut self, value: T) -> Option<u32> {
        let handle = (self.next != 0).then_some(self.next)?;
        self.next = handle.checked_add(1).unwrap_or(0);
        self.entries.insert(handle, value);
        Some(handle)
    }

    fn get(&self, handle: u32) -> Option<&T> {
        self.entries.get(&handle)
    }

    fn remove(&mut self, handle: u32) -> Option<T> {
        self.entries.remove(&handle)
    }

    /// Return a value `remove` lent out to the handle it came from.
    fn restore(&mut self, handle: u32, value: T) {
        self.entries.insert(handle, value);
    }
}

/// Registered input regions, uninitialized until the private JS caller writes every
/// byte (`copyInput`, the resource table). Engine calls read an input only after
/// that write; `geospec_engine_native_input_free` drops a region without reading it.
#[derive(Debug)]
struct InputAllocations {
    entries: BTreeMap<u32, Box<[MaybeUninit<u8>]>>,
}

impl InputAllocations {
    fn new() -> Self {
        Self {
            entries: BTreeMap::new(),
        }
    }

    fn allocate(&mut self, length: u32) -> Option<u32> {
        let mut bytes = Box::<[u8]>::new_uninit_slice(length as usize);
        let pointer = u32::try_from(bytes.as_mut_ptr() as usize).ok()?;
        if pointer == 0 || self.entries.contains_key(&pointer) {
            return None;
        }
        self.entries.insert(pointer, bytes);
        Some(pointer)
    }

    /// Return a region `take` lent out for one engine call.
    fn restore(&mut self, pointer: u32, bytes: Box<[MaybeUninit<u8>]>) {
        if pointer != 0 {
            self.entries.insert(pointer, bytes);
        }
    }

    fn take(&mut self, pointer: u32, length: u32) -> Option<Box<[MaybeUninit<u8>]>> {
        match (pointer, length) {
            (0, 0) => Some(Box::new_uninit_slice(0)),
            (0, _) => None,
            _ => {
                if self.entries.get(&pointer)?.len() != length as usize {
                    return None;
                }
                self.entries.remove(&pointer)
            }
        }
    }
}

// No `RefCell` borrow is held across an engine or OCCT call. A WASM trap
// unwinds no Rust frame, so a borrow held across one would stay held for the
// life of the instance, and every later call (the loader's input frees among
// them) would panic "RefCell already borrowed" and hide the trap. Engine calls
// therefore borrow their input region and engine out of the registries and
// return them afterwards: after a trap they are gone, never borrowed.
thread_local! {
    // Handles crossing the ABI are checked one-based arena indices. The arenas
    // stay thread-local because the configured engine is single-thread confined.
    static ENGINES: RefCell<Arena<Engine>> = RefCell::new(Arena::new());
    static RESULTS: RefCell<Arena<AbiResult>> = RefCell::new(Arena::new());
    // Pointers are accepted only while their exact Rust-owned allocation is
    // registered here. Private callers cannot transfer arbitrary malloc memory.
    static INPUTS: RefCell<InputAllocations> = RefCell::new(InputAllocations::new());
}

#[derive(Clone, Copy)]
struct InputBuffer {
    pointer: u32,
    length: u32,
}

fn result(value: Result<Vec<u8>, ProtocolError>) -> u32 {
    result_handle(AbiResult(value.map_err(Failure::from)))
}

fn failure(code: &str, message: &str) -> u32 {
    result_handle(AbiResult(Err(Failure {
        code: code.as_bytes().to_vec(),
        message: message.as_bytes().to_vec(),
    })))
}

fn result_handle(value: AbiResult) -> u32 {
    RESULTS.with(|results| results.borrow_mut().insert(value).unwrap_or(0))
}

fn with_input<R>(pointer: u32, length: u32, operation: impl FnOnce(&[u8]) -> R) -> Option<R> {
    let region = INPUTS.with(|inputs| inputs.borrow_mut().take(pointer, length))?;
    // SAFETY: the ABI caller wrote all `length` bytes before this engine call.
    let bytes = unsafe { std::slice::from_raw_parts(region.as_ptr().cast::<u8>(), region.len()) };
    let output = operation(bytes);
    INPUTS.with(|inputs| inputs.borrow_mut().restore(pointer, region));
    Some(output)
}

/// Run one engine call with the engine lent out of its arena.
fn with_engine(handle: u32, operation: impl FnOnce(&mut Engine) -> u32) -> u32 {
    let Some(mut engine) = ENGINES.with(|engines| engines.borrow_mut().remove(handle)) else {
        return failure("invalid-request", "GeoSpec engine handle is missing.");
    };
    let output = operation(&mut engine);
    ENGINES.with(|engines| engines.borrow_mut().restore(handle, engine));
    output
}

fn adopt(pointer: u32, length: u32) -> Option<Vec<u8>> {
    let bytes = INPUTS.with(|inputs| inputs.borrow_mut().take(pointer, length))?;
    // SAFETY: the ABI caller wrote all `length` bytes before transferring the input.
    Some(unsafe { bytes.assume_init() }.into_vec())
}

fn input_table(pointer: u32, count: u32) -> Option<Vec<InputBuffer>> {
    let byte_length = count.checked_mul(8)?;
    with_input(pointer, byte_length, |bytes| {
        bytes
            .chunks_exact(8)
            .map(|entry| InputBuffer {
                pointer: u32::from_le_bytes(entry[0..4].try_into().expect("exact chunk")),
                length: u32::from_le_bytes(entry[4..8].try_into().expect("exact chunk")),
            })
            .collect()
    })
}

fn adopt_inputs(descriptors: Option<&[InputBuffer]>) -> Option<Vec<Vec<u8>>> {
    let descriptors = descriptors?;
    let mut inputs = Vec::with_capacity(descriptors.len());
    let mut valid = true;
    for descriptor in descriptors {
        match adopt(descriptor.pointer, descriptor.length) {
            Some(input) => inputs.push(input),
            None => valid = false,
        }
    }
    valid.then_some(inputs)
}

fn invoke_engine(
    engine: u32,
    input: u32,
    input_length: u32,
    operation: impl FnOnce(&Engine, &[u8]) -> Result<Vec<u8>, ProtocolError>,
) -> u32 {
    with_input(input, input_length, |input| {
        with_engine(engine, |engine| result(operation(engine, input)))
    })
    .unwrap_or_else(|| {
        failure(
            "invalid-request",
            "GeoSpec control input allocation is missing.",
        )
    })
}

/// Allocate one exact-length Rust-owned binary input region.
#[no_mangle]
pub extern "C" fn geospec_engine_native_input_alloc(length: u32) -> u32 {
    if length == 0 {
        return 0;
    }
    INPUTS.with(|inputs| inputs.borrow_mut().allocate(length).unwrap_or(0))
}

/// Release an allocated input that was not transferred to an ingest call.
///
/// The private caller must return the exact registered pointer and length from
/// `geospec_engine_native_input_alloc` exactly once. Unknown pairs are ignored.
#[no_mangle]
pub extern "C" fn geospec_engine_native_input_free(pointer: u32, length: u32) {
    INPUTS.with(|inputs| {
        if let Ok(mut inputs) = inputs.try_borrow_mut() {
            drop(inputs.take(pointer, length));
        }
    });
}

/// Create the configured OCCT + Rust Manifold engine.
#[no_mangle]
pub extern "C" fn geospec_engine_native_engine_new() -> u32 {
    geospec_engine_native_engine_new_with_execution_permits(1)
}

/// Create an engine with a validated caller-inclusive allocation. No inner MT is enabled.
/// Zero reports invalid configuration without allocating an engine handle.
#[no_mangle]
pub extern "C" fn geospec_engine_native_engine_new_with_execution_permits(permits: u32) -> u32 {
    let Ok(config) = EngineConfig::entry().with_execution_permits(f64::from(permits)) else {
        return 0;
    };
    let Ok(engine) = create_engine(config) else {
        return 0;
    };
    ENGINES.with(|engines| engines.borrow_mut().insert(engine).unwrap_or(0))
}

/// Create an engine with validated caller-inclusive permits and host-selected limits.
///
/// The input holds strict JSON limits with camelCase names (`EngineConfig::with_limits_json`);
/// omitted limits keep their entry value. Returns a result handle whose success
/// bytes are the little-endian `u32` engine handle, or a typed refusal.
#[no_mangle]
pub extern "C" fn geospec_engine_native_engine_new_with_limits(
    permits: u32,
    input: u32,
    input_length: u32,
) -> u32 {
    with_input(input, input_length, |input| {
        let config = match EngineConfig::entry().with_execution_permits(f64::from(permits)) {
            Ok(config) => config,
            Err(message) => return failure("invalid-request", message),
        };
        let config = match config.with_limits_json(input) {
            Ok(config) => config,
            Err(error) => return result(Err(error)),
        };
        let engine = match create_engine(config) {
            Ok(engine) => engine,
            Err(message) => return failure("invalid-request", message),
        };
        match ENGINES.with(|engines| engines.borrow_mut().insert(engine)) {
            Some(handle) => result(Ok(handle.to_le_bytes().to_vec())),
            None => failure("limit-exceeded", "GeoSpec engine handles are exhausted."),
        }
    })
    .unwrap_or_else(|| {
        failure(
            "invalid-request",
            "GeoSpec limits input allocation is missing.",
        )
    })
}

/// Drop one retained engine.
#[no_mangle]
pub extern "C" fn geospec_engine_native_engine_drop(engine: u32) {
    ENGINES.with(|engines| {
        let _ = engines.borrow_mut().remove(engine);
    });
}

/// Canonicalize one borrowed control buffer.
#[no_mangle]
pub extern "C" fn geospec_engine_native_canonicalize(input: u32, input_length: u32) -> u32 {
    with_input(input, input_length, |input| {
        result(core_canonicalize(input))
    })
    .unwrap_or_else(|| {
        failure(
            "invalid-request",
            "GeoSpec control input allocation is missing.",
        )
    })
}

/// Non-mutating cumulative engine diagnostics. Snapshot copy traffic is excluded.
#[no_mangle]
pub extern "C" fn geospec_engine_native_observations(engine: u32) -> u32 {
    ENGINES.with(|engines| {
        let engines = engines.borrow();
        match engines.get(engine) {
            Some(engine) => result(Ok(engine.observations())),
            None => failure("invalid-request", "GeoSpec engine handle is missing."),
        }
    })
}

/// Admit one transferred legacy GSM1 buffer.
#[no_mangle]
pub extern "C" fn geospec_engine_native_ingest_mesh(
    engine: u32,
    request: u32,
    request_length: u32,
    mesh: u32,
    mesh_length: u32,
) -> u32 {
    let mesh = adopt(mesh, mesh_length);
    let Some(mesh) = mesh else {
        return failure(
            "invalid-request",
            "GeoSpec mesh input allocation is missing.",
        );
    };
    with_input(request, request_length, |request| {
        with_engine(engine, |engine| result(engine.ingest_mesh(request, &mesh)))
    })
    .unwrap_or_else(|| {
        failure(
            "invalid-request",
            "GeoSpec control input allocation is missing.",
        )
    })
}

/// Admit transferred primary and ordered resource buffers.
#[no_mangle]
pub extern "C" fn geospec_engine_native_ingest_subject(
    engine: u32,
    request: u32,
    request_length: u32,
    primary: u32,
    primary_length: u32,
    resources: u32,
    resource_count: u32,
) -> u32 {
    let primary = adopt(primary, primary_length);
    let descriptors = input_table(resources, resource_count);
    let adopted = adopt_inputs(descriptors.as_deref());
    let (Some(primary), Some(resources)) = (primary, adopted) else {
        return failure(
            "invalid-request",
            "GeoSpec binary input allocation is missing.",
        );
    };
    with_input(request, request_length, |request| {
        with_engine(engine, |engine| {
            result(engine.ingest_subject(request, &primary, resources))
        })
    })
    .unwrap_or_else(|| {
        failure(
            "invalid-request",
            "GeoSpec control input allocation is missing.",
        )
    })
}

/// Acquire the stable owner/generation token for one admitted subject.
#[no_mangle]
pub extern "C" fn geospec_engine_native_subject_handle(
    engine: u32,
    request: u32,
    request_length: u32,
) -> u32 {
    invoke_engine(engine, request, request_length, Engine::subject_handle)
}

/// Release one generation-checked subject and its retained analysis.
#[no_mangle]
pub extern "C" fn geospec_engine_native_release_subject(
    engine: u32,
    request: u32,
    request_length: u32,
) -> u32 {
    with_input(request, request_length, |request| {
        with_engine(engine, |engine| result(engine.release_subject(request)))
    })
    .unwrap_or_else(|| {
        failure(
            "invalid-request",
            "GeoSpec control input allocation is missing.",
        )
    })
}

/// Process one borrowed request through a retained engine.
#[no_mangle]
pub extern "C" fn geospec_engine_native_process_request(
    engine: u32,
    request: u32,
    request_length: u32,
) -> u32 {
    invoke_engine(engine, request, request_length, Engine::process_request)
}

/// Reserved ST binding control; never routed through the public GeoSpec parser.
#[no_mangle]
pub extern "C" fn geospec_engine_native_exact_cluster_candidate_control(
    engine: u32,
    request: u32,
    request_length: u32,
) -> u32 {
    invoke_engine(
        engine,
        request,
        request_length,
        Engine::process_exact_cluster_candidate_control,
    )
}

/// Normalize one borrowed request through a retained engine.
#[no_mangle]
pub extern "C" fn geospec_engine_native_canonical_plan(
    engine: u32,
    request: u32,
    request_length: u32,
) -> u32 {
    invoke_engine(engine, request, request_length, Engine::canonical_plan)
}

/// Evaluate one borrowed canonical plan through a retained engine.
#[no_mangle]
pub extern "C" fn geospec_engine_native_evaluate_plan(
    engine: u32,
    plan: u32,
    plan_length: u32,
) -> u32 {
    invoke_engine(engine, plan, plan_length, Engine::evaluate_plan)
}

/// Canonicalize and evaluate one borrowed one-claim request through a retained engine.
#[no_mangle]
pub extern "C" fn geospec_engine_native_evaluate_claim(
    engine: u32,
    request: u32,
    request_length: u32,
) -> u32 {
    invoke_engine(engine, request, request_length, Engine::evaluate_claim)
}

/// Return one when a result owns an error.
#[no_mangle]
pub extern "C" fn geospec_engine_native_result_is_error(result: u32) -> u32 {
    RESULTS.with(|results| {
        results
            .borrow()
            .get(result)
            .is_some_and(|result| result.0.is_err())
            .into()
    })
}

fn success_bytes(result: &AbiResult) -> Option<&[u8]> {
    result.0.as_ref().ok().map(Vec::as_slice)
}

fn failure_code(result: &AbiResult) -> Option<&[u8]> {
    result.0.as_ref().err().map(|error| error.code.as_slice())
}

fn failure_message(result: &AbiResult) -> Option<&[u8]> {
    result
        .0
        .as_ref()
        .err()
        .map(|error| error.message.as_slice())
}

fn result_length(result: u32, access: fn(&AbiResult) -> Option<&[u8]>) -> u32 {
    RESULTS.with(|results| {
        results
            .borrow()
            .get(result)
            .and_then(access)
            .and_then(|bytes| u32::try_from(bytes.len()).ok())
            .unwrap_or(0)
    })
}

/// Private synchronous wasm32 borrow, valid until the result is dropped.
/// The pointer does not transfer ownership and must never be freed as an input.
fn result_pointer(result: u32, access: fn(&AbiResult) -> Option<&[u8]>) -> u32 {
    RESULTS.with(|results| {
        results
            .borrow()
            .get(result)
            .and_then(access)
            .filter(|bytes| !bytes.is_empty())
            .and_then(|bytes| u32::try_from(bytes.as_ptr() as usize).ok())
            .unwrap_or(0)
    })
}

#[no_mangle]
pub extern "C" fn geospec_engine_native_result_length(result: u32) -> u32 {
    result_length(result, success_bytes)
}

#[no_mangle]
pub extern "C" fn geospec_engine_native_result_pointer(result: u32) -> u32 {
    result_pointer(result, success_bytes)
}

#[no_mangle]
pub extern "C" fn geospec_engine_native_result_code_length(result: u32) -> u32 {
    result_length(result, failure_code)
}

#[no_mangle]
pub extern "C" fn geospec_engine_native_result_code_pointer(result: u32) -> u32 {
    result_pointer(result, failure_code)
}

#[no_mangle]
pub extern "C" fn geospec_engine_native_result_message_length(result: u32) -> u32 {
    result_length(result, failure_message)
}

#[no_mangle]
pub extern "C" fn geospec_engine_native_result_message_pointer(result: u32) -> u32 {
    result_pointer(result, failure_message)
}

/// Drop one owned success/error result.
#[no_mangle]
pub extern "C" fn geospec_engine_native_result_drop(result: u32) {
    RESULTS.with(|results| {
        let _ = results.borrow_mut().remove(result);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn error_code(handle: u32) -> Vec<u8> {
        RESULTS.with(|results| {
            failure_code(results.borrow().get(handle).expect("result handle"))
                .expect("an error result")
                .to_vec()
        })
    }

    /// A trap leaves whatever an engine call borrowed borrowed. No registry is
    /// borrowed during the call, so the loader's cleanup after a trap (an input
    /// free) cannot panic. Host addresses do not fit the wasm32 ABI, so this
    /// uses the empty input, which takes the same path.
    #[test]
    fn no_registry_is_borrowed_during_an_engine_call() {
        let seen = with_input(0, 0, |bytes| {
            assert!(INPUTS.with(|inputs| inputs.try_borrow_mut().is_ok()));
            assert!(ENGINES.with(|engines| engines.try_borrow_mut().is_ok()));
            geospec_engine_native_input_free(0, 0);
            bytes.len()
        });
        assert_eq!(seen, Some(0));
        let handle = with_engine(u32::MAX, |_| unreachable!("no engine is registered"));
        assert!(INPUTS.with(|inputs| inputs.try_borrow_mut().is_ok()));
        geospec_engine_native_result_drop(handle);
    }

    /// A free never panics, even while the registry is borrowed.
    #[test]
    fn an_input_free_never_panics() {
        INPUTS.with(|inputs| {
            let _held = inputs.borrow();
            geospec_engine_native_input_free(8, 4);
        });
    }

    /// Limits refuse before any engine exists: an unregistered input, malformed
    /// JSON (the empty input) and invalid permits each return a typed failure.
    #[test]
    fn invalid_engine_limits_are_typed_failures() {
        for (handle, code) in [
            (
                geospec_engine_native_engine_new_with_limits(1, 8, 4),
                &b"invalid-request"[..],
            ),
            (
                geospec_engine_native_engine_new_with_limits(1, 0, 0),
                b"invalid-json",
            ),
            (
                geospec_engine_native_engine_new_with_limits(0, 0, 0),
                b"invalid-request",
            ),
        ] {
            assert_eq!(error_code(handle), code);
            geospec_engine_native_result_drop(handle);
        }
        assert!(ENGINES.with(|engines| engines.borrow().entries.is_empty()));
    }

    #[test]
    fn a_missing_engine_is_a_typed_failure() {
        let handle = with_engine(u32::MAX, |_| unreachable!("no engine is registered"));
        assert_eq!(error_code(handle), b"invalid-request");
        geospec_engine_native_result_drop(handle);
    }
}
