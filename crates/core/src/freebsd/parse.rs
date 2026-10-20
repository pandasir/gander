//! Pure conversions for the FreeBSD backend.
//!
//! FreeBSD's `kinfo_proc` arrives already decoded (unlike Darwin's, which has to
//! be picked out of raw offsets), so there is much less parsing here than in
//! `linux::parse`. What is left is the set of small translations that are easy to
//! get wrong and that tests can pin down without a live kernel.

use crate::row::CommBuf;
use crate::system::{CpuTimes, SwapInfo};

/// Convert one `kern.cp_*` group into [`CpuTimes`].
///
/// FreeBSD's order is `CP_USER, CP_NICE, CP_SYS, CP_INTR, CP_IDLE`. Mapping it
/// onto the shared struct means:
///
/// * `system` = `CP_SYS`, `irq` = `CP_INTR` (interrupt time is accounted
///   separately from system time),
/// * `iowait` stays 0: FreeBSD has no such counter, and it charges I/O wait to
///   `CP_IDLE`. Reporting a computed guess would be worse than reporting the
///   kernel's own answer,
/// * `steal`, `guest` and `guest_nice` stay 0: there are no jails-aware steal or
///   guest counters to read.
///
/// The consequence is visible in the CPU bar: a machine blocked on a slow disk
/// shows as idle here where Linux would show `iowait`. That is what the kernel
/// says, not a bug in the mapping.
pub fn cpu_times(v: &[i64; 5]) -> CpuTimes {
    let n = |x: i64| -> u64 { x.max(0) as u64 };
    CpuTimes {
        user: n(v[0]),
        nice: n(v[1]),
        system: n(v[2]),
        idle: n(v[4]),
        iowait: 0,
        irq: n(v[3]),
        softirq: 0,
        steal: 0,
        guest: 0,
        guest_nice: 0,
    }
}

/// Swap from `kvm_getswapinfo`'s page counts.
///
/// `ksw_total` and `ksw_used` are in **pages**. FreeBSD's `swapinfo(8)` scales
/// them by `pagesize`; `htop`'s FreeBSD backend multiplies by 1024 instead, which
/// reports swap in kilobytes while labelling it bytes. Doing it the way
/// `swapinfo` does is the only choice that matches another tool on the same
/// machine.
pub fn swap_from_pages(total: u64, used: u64, page_size: u64) -> SwapInfo {
    let total = total.saturating_mul(page_size);
    let used = used.saturating_mul(page_size).min(total);
    SwapInfo {
        total,
        used,
        free: total - used,
    }
}

/// `MAXSLP`, from `sys/vmmeter.h`: the sleep length past which `ps` calls a
/// sleeping process idle.
pub const MAXSLP: u32 = 20;

/// `ki_stat` (plus the sleep flags) to the state letter `ps` prints.
///
/// The mapping is **not** `b'I' + (stat - 1)`, and it is not even a function of
/// `ki_stat` alone. `ps` (`bin/ps/print.c`, `state`) uses:
///
/// | `ki_stat` | value | letter |
/// |---|---|---|
/// | `SIDL` | 1 | `R` |
/// | `SRUN` | 2 | `R` |
/// | `SSLEEP` | 3 | `D` if uninterruptible, else `I` when it has slept ≥ 20 s, else `S` |
/// | `SSTOP` | 4 | `T` |
/// | `SZOMB` | 5 | `Z` |
/// | `SWAIT` | 6 | `W` |
/// | `SLOCK` | 7 | `L` |
///
/// Three of those are traps:
///
/// * **`SIDL` is `R`, not `I`.** It means "being created by `fork`", not "idle".
///   The arithmetic mapping produced `I` for a brand-new process and `U` for a
///   running one.
/// * **`SSLEEP` splits three ways.** The `D`/`S` distinction is `TDF_SINTR`, and
///   `D` is the state that explains a hung machine: a process blocked on disk is
///   uninterruptible and cannot be signalled away. Collapsing it into `S` hides
///   that entirely. The `I` case is the long-sleeping idle thread.
/// * **`SLOCK` is `L`**, a letter Linux has no equivalent for, and not `W`.
///
/// The trailing `+`/`N`/`s`/`<`/`L` flags `ps` appends are not part of the state
/// and are not produced here; `STATE` is one character on every platform.
pub fn state_letter(stat: i8, interruptible: bool, slptime: u32) -> u8 {
    match stat {
        1 | 2 => b'R', // SIDL, SRUN
        3 => {
            if !interruptible {
                b'D'
            } else if slptime >= MAXSLP {
                b'I'
            } else {
                b'S'
            }
        }
        4 => b'T', // SSTOP
        5 => b'Z', // SZOMB
        6 => b'W', // SWAIT
        7 => b'L', // SLOCK
        _ => b'?',
    }
}

/// `kinfo_proc.ki_pri.pri_class` to the `SCHED_POLICY` column's vocabulary.
///
/// The column exists to distinguish scheduling *policies*, and FreeBSD's classes
/// are a superset of Linux's: `timeshare` is roughly `other`, `idle` is `idle`,
/// and `realtime` covers what Linux splits into `fifo` and `rr`. The mapping is
/// therefore deliberately lossy in the same direction on both platforms: a reader
/// asking "is this process pinned real-time" gets the same answer.
pub fn sched_policy(pri_class: u8) -> u8 {
    match pri_class {
        // Interrupt thread: a kernel thread is not a user-scheduled policy, and
        // FreeBSD has no vocabulary for it in this column. It reports `other`;
        // `RowFlags::KERNEL_THREAD` is what marks the row, and inventing a
        // policy name for it would make `SCHED_POLICY` mean two things at once.
        1 => 0,
        2 => 1, // realtime -> fifo
        3 => 0, // timeshare -> other
        4 => 5, // idle
        _ => 0,
    }
}

/// A command name, bounded without allocating.
///
/// `ki_comm` is capped at `MAXCOMLEN` by the kernel, so this never truncates in
/// practice; the clamp is for a longer value from a future kernel.
///
/// The truncation is explicit rather than relying on `ArrayString::try_push_str`,
/// which **rejects** a string that does not fit and leaves the buffer empty. That
/// failure mode was live here: a 200-byte name produced an empty `COMM` column
/// rather than a shortened one, and an empty command name looks exactly like a
/// process the sampler could not identify.
pub fn comm_buf(text: &str) -> CommBuf {
    let mut out = CommBuf::new();
    const MAX: usize = 64;
    let end = if text.len() <= MAX {
        text.len()
    } else {
        // Never split a UTF-8 sequence: a name is bytes from the kernel and is
        // usually ASCII, but a thread name can carry anything.
        let mut i = MAX;
        while i > 0 && !text.is_char_boundary(i) {
            i -= 1;
        }
        i
    };
    if out.try_push_str(&text[..end]).is_err() {
        out.clear();
        for ch in text.chars() {
            if out.try_push(ch).is_err() {
                break;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_order_maps_cp_sys_to_system_and_cp_intr_to_irq() {
        // `kern.cp_time` on this VM: user=3411 nice=0 sys=1158 intr=215 idle=165658
        let t = cpu_times(&[3411, 0, 1158, 215, 165_658]);
        assert_eq!(t.user, 3411);
        assert_eq!(t.system, 1158);
        assert_eq!(t.irq, 215);
        assert_eq!(t.idle, 165_658);
        assert_eq!(t.iowait, 0, "FreeBSD has no iowait counter");
        // Every non-idle counter plus idle, and nothing double-counted.
        assert_eq!(
            t.total(),
            t.user + t.nice + t.system + t.idle + t.iowait + t.irq + t.softirq
        );
        assert_eq!(t.total(), 3411 + 1158 + 165_658 + 215);
        assert_eq!(t.busy(), t.total() - t.idle);
    }

    #[test]
    fn a_negative_counter_is_clamped_not_wrapped() {
        // A kernel counter cannot really be negative; a decode error can produce
        // one, and wrapping it into u64 would report 18 exabytes of user time.
        let t = cpu_times(&[-1, 0, 0, 0, 0]);
        assert_eq!(t.user, 0);
        assert_eq!(t.total(), 0);
    }

    #[test]
    fn state_letters_match_ps() {
        // SIDL ("being created by fork") is R, not I.
        assert_eq!(state_letter(1, false, 0), b'R');
        assert_eq!(state_letter(2, false, 0), b'R');
        // SSLEEP splits three ways on TDF_SINTR and ki_slptime.
        assert_eq!(state_letter(3, false, 0), b'D', "uninterruptible sleep");
        assert_eq!(state_letter(3, true, 0), b'S', "interruptible sleep");
        assert_eq!(state_letter(3, true, 19), b'S', "just under MAXSLP");
        assert_eq!(state_letter(3, true, 20), b'I', "MAXSLP is inclusive");
        assert_eq!(state_letter(4, false, 0), b'T');
        assert_eq!(state_letter(5, false, 0), b'Z');
        assert_eq!(state_letter(6, false, 0), b'W');
        // SLOCK is L, a letter Linux never produces.
        assert_eq!(state_letter(7, false, 0), b'L');
        // Unknown values must not silently become a real state letter.
        assert_eq!(state_letter(0, false, 0), b'?');
        assert_eq!(state_letter(99, false, 0), b'?');
        assert_eq!(state_letter(-1, false, 0), b'?');
    }

    #[test]
    fn swap_pages_are_scaled_by_the_page_size() {
        // A 256 MiB swap file on 4 KiB pages is 65536 pages. Scaling by 1024 (the
        // htop FreeBSD bug) would report 64 MiB; not scaling at all, 64 KiB.
        let s = swap_from_pages(65_536, 16_384, 4096);
        assert_eq!(s.total, 256 * 1024 * 1024);
        assert_eq!(s.used, 64 * 1024 * 1024);
        assert_eq!(s.free, 192 * 1024 * 1024);
        assert_eq!(s.total, s.used + s.free);
    }

    #[test]
    fn used_swap_cannot_exceed_total() {
        let s = swap_from_pages(100, 200, 4096);
        assert_eq!(s.used, s.total);
        assert_eq!(s.free, 0);
    }

    #[test]
    fn scheduling_classes_map_to_the_shared_vocabulary() {
        // A kernel thread is not a scheduling policy; the row flag says so.
        assert_eq!(sched_policy(1), 0);
        assert_eq!(sched_policy(2), 1); // realtime -> fifo
        assert_eq!(sched_policy(3), 0); // timeshare -> other
        assert_eq!(sched_policy(4), 5); // idle
        assert_eq!(sched_policy(99), 0, "an unknown class is 'other'");
    }

    #[test]
    fn comm_is_truncated_by_the_buffer_not_by_a_byte_slice() {
        let long = "x".repeat(200);
        let comm = comm_buf(&long);
        assert_eq!(comm.len(), 64);
        assert!(comm.as_str().is_char_boundary(comm.len()));
    }
}
