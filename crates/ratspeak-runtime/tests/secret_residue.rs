//! Owned identity keys and recovery phrases leave no copy in freed heap memory
//! when dropped or read from disk, also when a file is refused. A recording allocator
//! copies every block the checking thread frees, and each check searches
//! those copies for its secret.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::{Cell, UnsafeCell};
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};

use ratspeak_runtime::{lxmf, vault};
use rns_identity::identity::Identity;

const RECORD_CAPACITY: usize = 4 << 20;

struct Recording(UnsafeCell<[u8; RECORD_CAPACITY]>);

// Written only by the armed thread while armed, read only after disarming.
unsafe impl Sync for Recording {}

static RECORDING: Recording = Recording(UnsafeCell::new([0; RECORD_CAPACITY]));
static RECORDED: AtomicUsize = AtomicUsize::new(0);
static OVERFLOWED: AtomicBool = AtomicBool::new(false);
static SERIAL: Mutex<()> = Mutex::new(());

thread_local! {
    static ARMED: Cell<bool> = const { Cell::new(false) };
}

fn armed() -> bool {
    ARMED.try_with(Cell::get).unwrap_or(false)
}

struct RecordingAllocator;

unsafe impl GlobalAlloc for RecordingAllocator {
    // Zeroed while armed, so a recorded block holds only what the check wrote.
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if armed() {
            unsafe { System.alloc_zeroed(layout) }
        } else {
            unsafe { System.alloc(layout) }
        }
    }

    // The default `realloc` frees through here, so blocks left behind by
    // growth are recorded too.
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if armed() {
            let start = RECORDED.load(Ordering::Relaxed);
            let end = start + 4 + layout.size();
            if end <= RECORD_CAPACITY {
                let base = RECORDING.0.get().cast::<u8>();
                unsafe {
                    base.add(start)
                        .cast::<[u8; 4]>()
                        .write_unaligned((layout.size() as u32).to_le_bytes());
                    std::ptr::copy_nonoverlapping(ptr, base.add(start + 4), layout.size());
                }
                RECORDED.store(end, Ordering::Relaxed);
            } else {
                OVERFLOWED.store(true, Ordering::Relaxed);
            }
        }
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: RecordingAllocator = RecordingAllocator;

fn serial() -> MutexGuard<'static, ()> {
    SERIAL.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Runs `f` and returns its result with a copy of every block it freed.
fn freed_by<R>(f: impl FnOnce() -> R) -> (R, Vec<Vec<u8>>) {
    RECORDED.store(0, Ordering::Relaxed);
    OVERFLOWED.store(false, Ordering::Relaxed);
    ARMED.with(|armed| armed.set(true));
    let result = f();
    ARMED.with(|armed| armed.set(false));
    assert!(
        !OVERFLOWED.load(Ordering::Relaxed),
        "freed blocks exceeded the recording"
    );
    let len = RECORDED.load(Ordering::Relaxed);
    // SAFETY: disarmed, so nothing writes the recording while it is read.
    let recorded = unsafe { std::slice::from_raw_parts(RECORDING.0.get().cast::<u8>(), len) };
    let mut blocks = Vec::new();
    let mut at = 0;
    while at < len {
        let size = u32::from_le_bytes(recorded[at..at + 4].try_into().unwrap()) as usize;
        blocks.push(recorded[at + 4..at + 4 + size].to_vec());
        at += 4 + size;
    }
    (result, blocks)
}

/// Each freed block holding 16 consecutive bytes of `secret`, by `name`.
fn residue(freed: &[Vec<u8>], name: &str, secret: &[u8]) -> Vec<String> {
    let windows: HashSet<&[u8]> = secret.windows(16).collect();
    freed
        .iter()
        .filter(|block| block.windows(16).any(|window| windows.contains(window)))
        .map(|block| format!("{name} in a freed {}-byte block", block.len()))
        .collect()
}

fn pseudo_random<const N: usize>(seed: u64) -> [u8; N] {
    let mut state = seed;
    let mut out = [0u8; N];
    for byte in &mut out {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        *byte = (state >> 24) as u8;
    }
    out
}

// A key file with bytes after the key is refused, and the key it starts with
// is not left behind, whatever the file's size.
#[test]
fn an_oversize_key_file_frees_no_key() {
    let _serial = serial();
    let dir = tempfile::tempdir().unwrap();
    let key: [u8; 64] = pseudo_random(1);
    let mut found = Vec::new();
    for extra in [1, 36, 4096, 1 << 20] {
        let path = dir.path().join("identity");
        std::fs::write(&path, [&key[..], &vec![0x5a; extra]].concat()).unwrap();

        let (refused, freed) = freed_by(|| vault::read_plain_identity_key(&path).is_err());

        assert!(refused, "{extra} extra bytes");
        found.extend(residue(&freed, &format!("{extra} extra bytes: key"), &key));
    }
    assert!(found.is_empty(), "{found:?}");
}

// Another identity's contact card reads its key file for the public key, and
// leaves no copy of the private key.
#[test]
fn a_contact_card_from_the_profile_frees_no_key() {
    let _serial = serial();
    let root = tempfile::tempdir().unwrap();
    let key: [u8; 64] = pseudo_random(2);
    let identity = Identity::from_private_key(&key).unwrap();
    let hash = hex::encode(identity.hash);
    let id_dir = root.path().join("identities").join(&hash);
    std::fs::create_dir_all(&id_dir).unwrap();
    std::fs::write(id_dir.join("identity"), key).unwrap();

    let (public_key, freed) =
        freed_by(|| lxmf::contact_card_public_key_from_profile(root.path(), &hash));

    assert_eq!(public_key, Some(identity.get_public_key()));
    let found = residue(&freed, "key", &key);
    assert!(found.is_empty(), "{found:?}");
}

// A plain identity opens with its key and recovery phrase and leaves neither
// behind; a phrase file that is damaged or too large is refused, also without
// leaving the phrase behind.
#[test]
fn a_plain_identitys_phrase_and_key_are_read_without_residue() {
    let _serial = serial();
    let key: [u8; 64] = pseudo_random(3);
    let phrase =
        "orbit velvet lantern mosaic ember quarry tundra violet walnut zephyr bramble cinder";
    let padding = vec![b' '; vault::MAX_SEED_BYTES];
    let mut found = Vec::new();
    for (name, contents, opens) in [
        ("whole", phrase.as_bytes().to_vec(), true),
        ("damaged", [phrase.as_bytes(), &[0xff]].concat(), false),
        ("too large", [phrase.as_bytes(), &padding].concat(), false),
    ] {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("identity"), key).unwrap();
        // The plaintext phrase's on-disk name.
        std::fs::write(dir.path().join("identity.seed"), &contents).unwrap();
        let job = vault::try_acquire_job().unwrap();

        let (opened, freed) = freed_by(|| {
            job.open_identity(dir.path(), None)
                .map(|identity| identity.mnemonic() == Some(phrase) && identity.key() == &key)
        });

        assert_eq!(opened.ok(), opens.then_some(true), "{name}");
        found.extend(residue(
            &freed,
            &format!("{name}: phrase"),
            phrase.as_bytes(),
        ));
        found.extend(residue(&freed, &format!("{name}: key"), &key));
    }
    assert!(found.is_empty(), "{found:?}");
}

// Record only the returned owner's destruction: derivation's internal
// allocations belong to the seed implementation's own tests. Boxing the key
// lets the allocator observe its storage immediately after Drop.
#[cfg(feature = "seed")]
#[test]
fn derived_identity_key_owner_wipes_on_drop() {
    let _serial = serial();
    let key = ratspeak_runtime::derive_identity_key_from_phrase(
        "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about",
    )
    .unwrap();
    let expected = key.to_vec();
    let key = std::hint::black_box(Box::new(key));

    let ((), freed) = freed_by(|| drop(key));

    assert!(!freed.is_empty(), "key storage was not recorded");
    let found = residue(&freed, "derived key", &expected);
    assert!(found.is_empty(), "{found:?}");
}

#[cfg(feature = "seed")]
#[test]
fn recoverable_key_owners_wipe_on_drop() {
    let _serial = serial();
    let (phrase, key) = ratspeak_runtime::generate_recoverable_key().unwrap();
    let expected_phrase = phrase.as_bytes().to_vec();
    let expected_key = key.to_vec();
    let key = std::hint::black_box(Box::new(key));

    let ((), freed) = freed_by(|| drop((phrase, key)));

    assert!(freed.len() >= 2, "phrase and key storage were not recorded");
    let mut found = residue(&freed, "generated phrase", &expected_phrase);
    found.extend(residue(&freed, "generated key", &expected_key));
    assert!(found.is_empty(), "{found:?}");
}

#[cfg(all(
    feature = "hardware",
    not(any(target_os = "android", target_os = "ios"))
))]
#[test]
fn provisioned_phrase_owner_wipes_on_drop() {
    let _serial = serial();
    let phrase =
        "orbit velvet lantern mosaic ember quarry tundra violet walnut zephyr bramble cinder";
    let provisioned = ratspeak_runtime::hardware::HwProvisioned {
        hash: "identity".to_owned(),
        lxmf_hash: "destination".to_owned(),
        mnemonic: Some(phrase.to_owned().into()),
    };

    let ((), freed) = freed_by(|| drop(provisioned));

    assert!(freed.iter().any(|block| block.len() >= phrase.len()));
    let found = residue(&freed, "provisioned phrase", phrase.as_bytes());
    assert!(found.is_empty(), "{found:?}");
}
