//! CPU affinity: which cores a process may run on.
//!
//! The two supported platforms disagree about what affinity *is*, so this module
//! models the platform honestly instead of pretending they match:
//!
//! * Linux has hard affinity. `sched_getaffinity` reads the mask, and the mask is
//!   enforced by the scheduler, so it can be set without privilege on your own
//!   processes.
//! * macOS has only a *hint*. `thread_policy_set(THREAD_AFFINITY_POLICY)` tags a
//!   thread with an affinity group, and the scheduler uses it as a preference.
//!   There is no per-process mask, and reading it back is not supported. Leaving
//!   the mask unset on macOS is therefore correct, not a missing feature.
//!
//! The mask is a `Vec<u64>`, one bit per core, which is how Linux stores it. The
//! platform glue lives behind `#[cfg]` at the bottom so the rendering and the
//! toggle logic can be tested anywhere.

/// A set of CPUs, one bit per core.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CpuMask {
    words: Vec<u64>,
}

impl CpuMask {
    /// An empty mask, sized for `cores` cores.
    pub fn empty(cores: usize) -> Self {
        Self {
            words: vec![0; cores.div_ceil(64).max(1)],
        }
    }

    /// Every core set, sized for `cores` cores.
    pub fn all(cores: usize) -> Self {
        let mut mask = Self::empty(cores);
        for core in 0..cores {
            mask.set(core, true);
        }
        mask
    }

    pub fn get(&self, core: usize) -> bool {
        let (word, bit) = (core / 64, core % 64);
        self.words.get(word).is_some_and(|w| w & (1 << bit) != 0)
    }

    pub fn set(&mut self, core: usize, on: bool) {
        let (word, bit) = (core / 64, core % 64);
        if word >= self.words.len() {
            self.words.resize(word + 1, 0);
        }
        if on {
            self.words[word] |= 1 << bit;
        } else {
            self.words[word] &= !(1 << bit);
        }
    }

    pub fn toggle(&mut self, core: usize) {
        let on = self.get(core);
        self.set(core, !on);
    }

    /// Flatten to bytes, the form `sched_setaffinity` wants.
    pub fn to_bytes(&self) -> Vec<u8> {
        self.words
            .iter()
            .flat_map(|w| w.to_ne_bytes())
            .collect()
    }

    /// Build from raw bytes, as returned by `sched_getaffinity`.
    pub fn from_bytes(bytes: &[u8], cores: usize) -> Self {
        let mut mask = Self::empty(cores);
        for core in 0..cores {
            let byte = core / 8;
            if byte < bytes.len() && bytes[byte] & (1 << (core % 8)) != 0 {
                mask.set(core, true);
            }
        }
        mask
    }

    pub fn count(&self) -> usize {
        self.words.iter().map(|w| w.count_ones() as usize).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.count() == 0
    }
}

/// Whether this build can change affinity at all.
pub fn supported() -> bool {
    cfg!(target_os = "linux")
}

/// Read the affinity mask of `pid`.
///
/// Returns `Ok(None)` where the platform has no readable mask: that is a fact
/// about the OS, not an error.
pub fn read(pid: u32, cores: usize) -> Result<Option<CpuMask>, String> {
    imp::read(pid, cores)
}

/// Write the affinity mask of `pid`.
pub fn write(pid: u32, mask: &CpuMask) -> Result<(), String> {
    if mask.is_empty() {
        // A zero mask is rejected by the kernel, and it would mean "no core at
        // all", which is not a state a process can be in.
        return Err("refusing to set an empty CPU mask".to_string());
    }
    imp::write(pid, mask)
}

#[cfg(target_os = "linux")]
mod imp {
    use super::CpuMask;
    use std::io;

    pub fn read(pid: u32, cores: usize) -> Result<Option<CpuMask>, String> {
        let mut bytes = vec![0u8; cores.div_ceil(64).max(1) * 8];
        // SAFETY: the buffer is `bytes.len()` bytes and the length matches.
        let rc = unsafe {
            libc::sched_getaffinity(
                pid as libc::pid_t,
                bytes.len(),
                bytes.as_mut_ptr().cast(),
            )
        };
        if rc != 0 {
            return Err(io::Error::last_os_error().to_string());
        }
        Ok(Some(CpuMask::from_bytes(&bytes, cores)))
    }

    pub fn write(pid: u32, mask: &CpuMask) -> Result<(), String> {
        let bytes = mask.to_bytes();
        // SAFETY: the buffer is `bytes.len()` bytes and the length matches.
        let rc = unsafe {
            libc::sched_setaffinity(
                pid as libc::pid_t,
                bytes.len(),
                bytes.as_ptr().cast(),
            )
        };
        if rc == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error().to_string())
        }
    }
}

#[cfg(not(target_os = "linux"))]
mod imp {
    use super::CpuMask;

    pub fn read(_pid: u32, _cores: usize) -> Result<Option<CpuMask>, String> {
        // macOS has no per-process mask. Returning `None` rather than an error
        // lets the panel say "not supported here" instead of looking broken.
        Ok(None)
    }

    pub fn write(_pid: u32, _mask: &CpuMask) -> Result<(), String> {
        Err(
            "this platform has no per-process CPU affinity mask; \
             macOS only offers a scheduler hint"
                .to_string(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bits_round_trip() {
        let mut m = CpuMask::empty(16);
        assert_eq!(m.count(), 0);
        m.set(0, true);
        m.set(15, true);
        assert!(m.get(0) && m.get(15));
        assert!(!m.get(7));
        assert_eq!(m.count(), 2);

        m.toggle(15);
        assert!(!m.get(15));
        m.toggle(15);
        assert!(m.get(15));
    }

    #[test]
    fn all_covers_every_core() {
        for cores in [1usize, 8, 64, 65, 128] {
            let m = CpuMask::all(cores);
            assert_eq!(m.count(), cores, "cores={cores}");
            assert!(m.get(cores - 1));
        }
    }

    #[test]
    fn a_mask_beyond_one_word_works() {
        let mut m = CpuMask::all(100);
        assert_eq!(m.count(), 100);
        m.set(99, false);
        assert_eq!(m.count(), 99);
        assert!(m.get(64));
    }

    #[test]
    fn bytes_round_trip() {
        let m = CpuMask::all(40);
        let back = CpuMask::from_bytes(&m.to_bytes(), 40);
        assert_eq!(m, back);
    }

    #[test]
    fn a_short_byte_buffer_is_not_trusted() {
        // The kernel may return fewer bytes than we asked for on an old machine.
        let back = CpuMask::from_bytes(&[0b0000_0101], 64);
        assert!(back.get(0));
        assert!(back.get(2));
        assert!(!back.get(8), "bits past the buffer must be clear, not garbage");
    }

    #[test]
    fn an_empty_mask_is_rejected() {
        let err = write(std::process::id(), &CpuMask::empty(4));
        assert!(err.is_err(), "an empty mask must never reach the kernel");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn reading_our_own_affinity_works_and_is_never_empty() {
        let cores = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1);
        let mask = read(std::process::id(), cores).expect("no error").expect("a mask");
        assert!(!mask.is_empty(), "a running process has at least one CPU");
        assert!(mask.count() <= cores);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn setting_our_own_affinity_round_trips() {
        let cores = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1);
        let original = read(std::process::id(), cores).unwrap().unwrap();

        // Pin to CPU 0 and read it back.
        let mut wanted = CpuMask::empty(cores);
        wanted.set(0, true);
        if write(std::process::id(), &wanted).is_ok() {
            let got = read(std::process::id(), cores).unwrap().unwrap();
            assert!(got.get(0), "the mask should now contain CPU 0");
            assert_eq!(got.count(), 1);
        }

        // Always restore, so the test does not slow the rest of the suite down.
        let _ = write(std::process::id(), &original);
    }

    #[cfg(not(target_os = "linux"))]
    #[test]
    fn platforms_without_a_mask_say_so() {
        assert!(!supported());
        assert_eq!(read(std::process::id(), 8), Ok(None));
        let err = write(std::process::id(), &CpuMask::all(8)).unwrap_err();
        assert!(err.contains("no per-process CPU affinity"), "got {err:?}");
    }
}
