//! Pure parsers for Darwin data sources.
//!
//! Only the values that arrive as raw bytes are parsed here
//! (`KERN_PROCARGS2`, `vm.swapusage`, `hw.memsize`). Everything else comes back
//! as typed structs from `sysctl`/`libproc`, so there is nothing stringly-typed
//! to get wrong.

/// Maximum argv bytes we will read from `KERN_PROCARGS2`. htop uses the
/// `kern.argmax` sysctl; this cap keeps a pathological process from allocating
/// hundreds of kilobytes during a scan.
pub const ARG_MAX_CAP: usize = 64 * 1024;

/// Parse a `KERN_PROCARGS2` buffer into a single displayable command line.
///
/// Layout (`<sys/sysctl.h>`):
/// ```text
/// int argc
/// char exec_path[]\0
/// char padding[]\0 ... (skipped)
/// char argv[0]\0 argv[1]\0 ... argv[argc-1]\0
/// char envp[]\0 ...
/// ```
///
/// Reading `argc` from the front and then walking NUL-terminated strings is the
/// only correct way; splitting on NUL alone would include the environment.
pub fn parse_procargs2(buf: &[u8]) -> Option<String> {
    if buf.len() < 4 {
        return None;
    }
    let argc = u32::from_ne_bytes([buf[0], buf[1], buf[2], buf[3]]) as usize;
    if argc == 0 || argc > 4096 {
        return None;
    }

    let mut rest = &buf[4..];

    // Skip the executable path.
    let nul = rest.iter().position(|b| *b == 0)?;
    rest = &rest[nul + 1..];

    // Skip alignment padding: a run of NULs before argv[0].
    while rest.first() == Some(&0) {
        rest = &rest[1..];
    }

    let mut parts: Vec<String> = Vec::with_capacity(argc.min(64));
    for _ in 0..argc {
        if rest.is_empty() {
            break;
        }
        let end = rest.iter().position(|b| *b == 0).unwrap_or(rest.len());
        if end == 0 {
            break;
        }
        parts.push(String::from_utf8_lossy(&rest[..end]).into_owned());
        rest = if end < rest.len() {
            &rest[end + 1..]
        } else {
            &[]
        };
    }

    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" "))
    }
}

/// Map a `kinfo_proc` `p_stat` value to an htop-style single-letter state.
///
/// Darwin uses the historical BSD `p_stat` constants, not Linux's letters.
pub fn state_letter(p_stat: i8) -> u8 {
    match p_stat {
        1 => b'I', // SIDL  - being created
        2 => b'R', // SRUN  - runnable
        3 => b'S', // SSLEEP - sleeping
        4 => b'T', // SSTOP - stopped
        5 => b'Z', // SZOMB - zombie
        _ => b'?',
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn procargs2(argc: u32, path: &str, argv: &[&str]) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend_from_slice(&argc.to_ne_bytes());
        buf.extend_from_slice(path.as_bytes());
        buf.push(0);
        // Alignment padding, as the kernel emits it.
        buf.extend_from_slice(&[0, 0, 0]);
        for (i, a) in argv.iter().enumerate() {
            if i > 0 {
                buf.push(0);
            }
            buf.extend_from_slice(a.as_bytes());
        }
        buf.push(0);
        // Environment must not leak into the command line.
        buf.extend_from_slice(b"HOME=/Users/x\0SHELL=/bin/zsh\0");
        buf
    }

    #[test]
    fn procargs2_basic() {
        let buf = procargs2(3, "/usr/bin/vim", &["vim", "-u", "NONE", "file.txt"][..3]);
        assert_eq!(parse_procargs2(&buf).as_deref(), Some("vim -u NONE"));
    }

    #[test]
    fn procargs2_stops_at_argc() {
        let buf = procargs2(2, "/bin/sh", &["sh", "-c", "echo hi"]);
        assert_eq!(
            parse_procargs2(&buf).as_deref(),
            Some("sh -c"),
            "argv beyond argc must not be included"
        );
    }

    #[test]
    fn procargs2_rejects_truncated_and_zero() {
        assert!(parse_procargs2(&[]).is_none());
        assert!(parse_procargs2(&[0, 0, 0]).is_none());
        assert!(parse_procargs2(&0u32.to_ne_bytes()).is_none(), "argc == 0");
    }

    #[test]
    fn procargs2_handles_args_with_spaces() {
        let buf = procargs2(2, "/bin/echo", &["echo", "hello world"]);
        assert_eq!(parse_procargs2(&buf).as_deref(), Some("echo hello world"));
    }

    #[test]
    fn state_letters() {
        assert_eq!(state_letter(2), b'R');
        assert_eq!(state_letter(3), b'S');
        assert_eq!(state_letter(4), b'T');
        assert_eq!(state_letter(5), b'Z');
        assert_eq!(state_letter(1), b'I');
        assert_eq!(state_letter(99), b'?');
    }
}
