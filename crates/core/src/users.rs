//! uid to user-name resolution.
//!
//! The `USER` column used to print the raw numeric uid, which is not what anyone
//! wants to read. `getpwuid_r` is POSIX and works on both backends, so it is
//! preferred over parsing `/etc/passwd` (which cannot see directory-service users
//! on macOS and would need to be re-implemented for every other platform).

use std::collections::HashMap;

use crate::Pid;

/// Cache of resolved user names, keyed by uid.
///
/// Lookups are cached because a scan resolves the same handful of uids thousands
/// of times, and `getpwuid_r` walks the directory service each call.
#[derive(Default)]
pub struct Users {
    by_uid: HashMap<u32, Name>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Name {
    Resolved(String),
    /// The uid has no passwd entry. Remembered so we do not retry every tick.
    Unknown,
}

impl Users {
    pub fn new() -> Self {
        Self::default()
    }

    /// Name for `uid`, falling back to the decimal number when there is no entry.
    pub fn name(&mut self, uid: u32) -> &str {
        let entry = self.by_uid.entry(uid).or_insert_with(|| match lookup(uid) {
            Some(name) => Name::Resolved(name),
            None => Name::Unknown,
        });

        match entry {
            Name::Resolved(name) => name.as_str(),
            Name::Unknown => {
                // Store the numeric form so the borrowed return is stable.
                *entry = Name::Resolved(uid.to_string());
                match entry {
                    Name::Resolved(name) => name.as_str(),
                    Name::Unknown => unreachable!(),
                }
            }
        }
    }

    /// Resolve without caching, for one-off use.
    pub fn name_of(uid: u32) -> String {
        lookup(uid).unwrap_or_else(|| uid.to_string())
    }

    pub fn len(&self) -> usize {
        self.by_uid.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_uid.is_empty()
    }
}

/// Ask the system for the login name of `uid`.
#[cfg(unix)]
fn lookup(uid: u32) -> Option<String> {
    // 1 KiB is the documented safe starting size; sysconf(_SC_GETPW_R_SIZE_MAX)
    // is advisory and sometimes returns -1, so it is only a hint.
    let mut buf = vec![0i8; 1024];

    loop {
        let mut pwd: libc::passwd = unsafe { std::mem::zeroed() };
        let mut result: *mut libc::passwd = std::ptr::null_mut();

        // SAFETY: `buf` is writable and `buf.len()` bytes long; `pwd` and `result`
        // are valid out-parameters. A non-zero return means it failed or the
        // buffer was too small.
        let rc = unsafe {
            libc::getpwuid_r(
                uid as libc::uid_t,
                &mut pwd,
                buf.as_mut_ptr().cast(),
                buf.len(),
                &mut result,
            )
        };

        if rc == libc::ERANGE && buf.len() < 1 << 20 {
            buf.resize(buf.len() * 2, 0);
            continue;
        }
        if rc != 0 || result.is_null() {
            return None;
        }

        // SAFETY: on success `pw_name` points into `buf`, which outlives this
        // block, and is NUL-terminated.
        let name = unsafe { std::ffi::CStr::from_ptr(pwd.pw_name) };
        return Some(name.to_string_lossy().into_owned());
    }
}

#[cfg(not(unix))]
fn lookup(_uid: u32) -> Option<String> {
    None
}

/// Pids whose owner is `uid`, for the user filter.
pub fn pids_of_user(rows: &[crate::row::Row], uid: u32) -> Vec<Pid> {
    rows.iter()
        .filter(|r| r.uid == uid)
        .map(|r| r.pid)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_root() {
        // uid 0 exists on every unix.
        assert_eq!(Users::name_of(0), "root");
    }

    #[test]
    fn resolves_the_current_user() {
        // SAFETY: `getuid` has no preconditions.
        let uid = unsafe { libc::getuid() };
        let name = Users::name_of(uid);
        assert!(!name.is_empty());
        assert_ne!(
            name,
            uid.to_string(),
            "current user should have a passwd entry"
        );
    }

    #[test]
    fn unknown_uid_falls_back_to_the_number() {
        // 4_000_000 is above the range any sane system allocates.
        assert_eq!(Users::name_of(4_000_000), "4,000,000".replace(',', ""));
    }

    #[test]
    fn cache_returns_a_stable_borrow() {
        let mut users = Users::new();
        let a = users.name(0).to_string();
        let b = users.name(0).to_string();
        assert_eq!(a, b);
        assert_eq!(users.len(), 1, "second lookup must hit the cache");
    }

    #[test]
    fn cache_caches_the_fallback_too() {
        let mut users = Users::new();
        assert_eq!(users.name(4_000_001), "4000001");
        assert_eq!(users.name(4_000_001), "4000001");
        assert_eq!(users.len(), 1);
    }
}
