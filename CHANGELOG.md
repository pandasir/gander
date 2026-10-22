# Changelog

Newest first. Every entry is the annotated tag message, verbatim: these notes are
not maintained here, they are *derived* from the tag. The same text is what
`git show v0.6.0` prints and what the GitHub release shows, because all three
read it from the same place.

The reasoning behind each milestone — why a feature is shaped the way it is, and
what a live kernel taught the implementation — is in [docs/ROADMAP.md](docs/ROADMAP.md).
The per-release notes below say what shipped; ROADMAP says why.

---

## v0.6.4 — 2026-10-22

A display bug, not a data bug. `snapshot.cpu_count()` read all 128 cores
correctly all along — `linux_live.rs` asserts it against
`sysconf(_SC_NPROCESSORS_ONLN)` — and then the system column drew 64 of them and
printed "64 cores" next to the grid. The number was the grid's capacity
presented as a fact about the hardware, so there was nothing on screen to suggest
a setting was responsible, and nothing in the docs or in `F2` setup to change.

Three limits, each of which hid the others

  `Config::system_cores_max` defaulted to 64, and the label printed the capped
  count rather than the machine's. Raising it to `0`, which its own doc comment
  described as "render every core", changed nothing: the grid was also capped at
  16 cells per row times four rows, so 64 glyphs was the ceiling no matter what
  the config said. And under `layout = "htop"` the same machine rendered *no
  header at all* — `draw_column_indices` skipped any meter taller than the rows
  that were left, and 128 cores want 65 rows per column, so the first meter
  failed the test and the entire header went blank.

  Fixing any one of the three alone would have left the other two in place, which
  is why the report read as "the core count is wrong" rather than as "the grid is
  capped".

What the column does now

  The grid takes its shape from the terminal: up to 16 cores per row, and as many
  rows as the height leaves after reserving room for MEM, SWP, LOAD, TASKS, RUN
  and THR. A 128-core box draws eight rows on a 40-row terminal, and the figures
  below the grid stay where they were.

  The label reports what is actually drawn, and names the total when they differ:

    128 cores          everything fits
    64/128 cores       a short terminal or a configured cap stopped the grid

  A truncated grid is fine. Reporting the truncation as the machine's core count
  was the bug, and `64/128` is the difference between a limit and a lie.

`system_cores_max` is now reachable, and defaults to "all"

  Default 0. `F2` -> Display cycles `0 / 16 / 32 / 64 / 128`, which is how the
  setting should have been reachable from the start: a config-file-only value
  that silently changes a displayed figure is indistinguishable from a wrong
  figure. `0` renders as "all cores" rather than "0 cores", because the sentinel
  means the opposite of what it says.

  An existing `config.toml` with `system_cores_max = 64` keeps its 64, and now
  shows `64/128 cores` instead of `64 cores`. That is the honest reading of a
  setting someone chose.

The htop header clips instead of disappearing

  A meter taller than the remaining rows is drawn as far as it goes. The
  renderers already stopped at `area.bottom()`; only the caller's `break` was
  discarding the whole thing. A one-row slice of a split CPU meter relabels
  itself `CPU` instead of keeping `Avg`, which asks "the average of what?" once
  the core rows are gone.

Tests: 128 cores render 128 glyphs; a 16-row terminal labels itself `N/128`; a
configured cap labels itself `64/128`; the figures below the grid survive; the
htop header is non-blank at 128 cores on 50- and 24-row terminals; the grid-cap
ladder wraps and recovers from a hand-edited value. 173 TUI, 146 core, 33 config
tests, clippy `-D warnings` clean, and the release binary reports 0.6.4.

<sub>[compare v0.6.3...v0.6.4](https://github.com/pandasir/gander/compare/v0.6.3...v0.6.4)</sub>

## v0.6.3 — 2026-10-22

A packaging release. Nothing about the process table changed; what changed is
that installing gander no longer means downloading a tarball and copying a
binary into ~/.local/bin by hand.

Debian, Ubuntu, Fedora, RHEL and Rocky now have a real package manager path

  https://pandasir.github.io/gander/ is a signed apt repository and a signed
  yum/dnf repository, served from a `repo` branch by GitHub Pages:

    sudo apt install gander
    sudo dnf install gander

  Upgrades arrive with the rest of the system. apt verifies InRelease, dnf runs
  with gpgcheck=1 and repo_gpgcheck=1, and the RPMs are signed as well as the
  metadata. The .deb files are not signed individually: apt authenticates the
  index, which carries each package's SHA-256, so signing the package too would
  protect nothing that the index does not.

The packages declare no dependencies, and that is the whole product

  Both are built from the static musl binary, so they install on Debian 12,
  Ubuntu 24.04, Rocky 9 and CentOS 7 alike. check-packages.sh asserts both
  halves of the claim -- the control/Requires fields are empty and readelf shows
  no NEEDED entries -- because either one can be made true while the other
  regresses. A package that quietly picked up libc6 >= 2.34 would install on the
  build machine and refuse to start on the user's.

Linux arm64, at last

  aarch64-unknown-linux-gnu and aarch64-unknown-linux-musl are built on
  GitHub's hosted arm64 runners, so the arm64 packages and the Homebrew Linux
  ARM install both stop compiling from source. v0.6.2 shipped no arm64 archive,
  which is why the formula still builds there; that becomes a prebuilt download
  in the next formula update.

What a live client taught the implementation

  Every one of these was found by running the thing rather than by reading it,
  and none would have been caught by a green CI run of the code alone. The
  orphan-branch path in the publish job had no `origin` remote, so the *first*
  release would have failed to push -- a path that runs once and is therefore
  never exercised again. sign-packages.sh checked the SIGPGP/SIGGPG tags, but
  rpm 4.14+ stores a header signature in RSAHEADER, so it reported "none" for a
  correctly signed package and would have passed one whose signing silently
  failed. check-packages.sh passed a relative RPM path through a chdir, which
  every local run missed because local runs used absolute paths. rpmbuild from
  Debian refuses to build for a foreign architecture at all, so each package is
  now built on a runner of its own architecture. linux-install echoed an
  undefined ${IMAGE} on its last line, failing after the archive had already
  downloaded, verified and run. `gh release upload` failed with "not a git
  repository" in a job with no checkout. The repository's client check could not
  resolve `host.docker.internal`, a Docker Desktop convenience absent on a Linux
  runner; and once it could, the arm64 client died with "exec format error"
  because the x86_64 runner had no QEMU handlers registered.

  Verified against a local HTTP server with real clients: apt install gander on
  debian:12 amd64 and arm64, dnf install gander on rockylinux:9 amd64 and arm64,
  a tampered .deb rejected with "Hash Sum mismatch", and a tampered repomd.xml
  rejected with "Bad GPG signature".

Signing key

  docs/SIGNING.md is the key's whole lifecycle. It is created once by a
  maintainer and stored as two repository secrets; nothing is generated in CI,
  because a key that changed every release would invalidate every user's
  imported copy. It has no expiry on purpose -- an expired key breaks every
  `apt update` on the expiry date, which is a worse failure than a long-lived
  key in a secret store.

Tests: 146 core, 33 config, 160 TUI, plus integration and doc tests;
cargo clippy --workspace --all-targets -- -D warnings is clean. The new packages
job runs on every pull request, and packaging/test-repo.sh exercises the whole
repository chain with a throwaway key.

<sub>[compare v0.6.2...v0.6.3](https://github.com/pandasir/gander/compare/v0.6.2...v0.6.3)</sub>

## v0.6.2 — 2026-10-21

This is a packaging release: no behaviour changed, but the project can now be installed from a source tarball without resolving a moving dependency graph.

crates.io-ready workspace

  The internal gander dependencies now carry a version alongside their path, which cargo requires before it will package any of the four crates. The executable is published as gander-bin because the gander crate name was taken by an unrelated, yanked project in 2024; cargo install gander-bin still installs a binary named gander.

  Cargo.lock is committed. The v0.6.1 source archive did not contain one, so cargo build --locked failed before compiling a line and Homebrew or AUR recipes could not pin the dependency graph. The same archive now builds reproducibly with the lockfile that CI and local tests use.

  README and docs/RELEASING.md now describe the four-crate publish order and the lockfile rule, so the next release cannot silently regress either one.

Packaging recipes

  packaging/ contains a Homebrew formula, an AUR PKGBUILD with .SRCINFO, and the checksums and commands needed to submit them. All point at the annotated v0.6.2 tag; the distribution metadata lives with the source rather than only in a tap or an AUR repository.

Tests: 145 core, 33 config, 151 TUI, plus integration and doc tests; cargo clippy --workspace --all-targets -- -D warnings is clean. The full suite runs outside the macOS sandbox, which denies KERN_PROC_ALL.

<sub>[compare v0.6.1...v0.6.2](https://github.com/pandasir/gander/compare/v0.6.1...v0.6.2)</sub>

## v0.6.1 — 2026-10-20

**cleanup — dead code deleted, docs corrected**

No new features. Three things that were lying about the code, and one small fix.

Dead code

  `capability` had ten constants and four were read by nothing but their own doc
  comments. `PROC_IO` was the worst: `true` on Linux and macOS, claiming
  per-process I/O counters were available, while no column existed to be gated by
  it. Removed along with `IOPRIO`, `SMAPS`, `PSI`, `DELAYACCT`, `THREADS`, `LIVE`.

  Darwin's `PrevRow` carried `disk_read`/`disk_written` behind
  `#[allow(dead_code)]` since M1, and the `proc_pid_rusage` call that filled them
  ran once per process per tick with the result thrown away. The attribute silenced
  the compiler, not the syscall. Both gone; `scan::rusage()` stays, tested and
  documented as unwired, for whoever builds the I/O columns.

Docs

  `DESIGN.md` §2 listed `sort.rs`, `filter.rs` and a `criterion` dependency that do
  not exist, and omitted the `darwin/` and `freebsd/` backends. §4.1 restated the
  `Field` enum from memory, including two variants deleted a milestone ago. §3 said
  "the two platforms" about memory accounting when there are three.

  These are now pointers to the source of truth plus the reasoning that is not in
  the code. A doc that restates code is a doc that will be wrong within a milestone.

`s` reports the actual `ptrace_scope`

  It said tracing "may" be forbidden. With the common `ptrace_scope = 1` the attach
  fails with EPERM for every pid on the machine including your own, which is not
  something "may" lets you debug. The panel now reads the file, prints the value,
  says what it means and how to change it. macOS names SIP, FreeBSD names truss.

Three test bugs, all the same mistake

  Asserting the machine instead of the contract: `devname(...) == "ttys000"` (broken
  CI for two pushes), and `rusage → non-zero` (a page-cached test binary has done no
  block I/O, so zero is correct). Both now check that the call succeeds and the data
  is readable, which holds on any host.

376 tests, zero clippy warnings, the Linux path re-run in a rust:1.89-slim
container, all five CI jobs green including the FreeBSD VM.

<sub>[compare v0.6.0...v0.6.1](https://github.com/pandasir/gander/compare/v0.6.0...v0.6.1)</sub>

## v0.6.0 — 2026-10-20

**M4 complete — FreeBSD, the third backend**

A third implementation of `PlatformSampler`, on a system with no `/proc`. The
whole port is one sysctl away from being cheap and several traps away from being
wrong; what follows is mostly the second part.

Why FreeBSD needed its own model rather than another `/proc` reader

  FreeBSD has no `/proc`. The data comes from `sysctl` and `libkvm`, the same pair
  htop's `freebsd/` uses. One `sysctl(KERN_PROC_PROC)` returns the whole table with
  CPU time, memory, RSS, thread count and the BSD identity fields, so the default
  column set costs a single syscall — fewer than Linux's one `openat` per process.

  It was written and verified on FreeBSD 14.5-RELEASE/arm64 in QEMU: the whole
  workspace suite runs inside the guest, plus `tests/freebsd_live.rs`, twenty tests
  that compare gander's figures against `sysctl(8)`, `ps(1)`, `swapinfo(8)` and
  `netstat(1)` in the same test, so a changing system cannot explain a
  discrepancy.

The five things a live run found that review had not

  `SSLEEP` is three states, not one. `ki_stat` is not a state letter and is not even
  a function of the state: `SIDL` (1) means "being created by fork" and `ps` prints
  `R` for it; `SSLEEP` splits into `D` (uninterruptible, `ki_tdflags & TDF_SINTR`
  clear), `I` (interruptible and asleep for `MAXSLP` = 20 s), and `S`; `SLOCK` is
  `L`. Mapping `1..7` onto `I R S T Z W ?` arithmetically reported `I` for a
  brand-new process and `S` for every sleeper — and collapsing `D` into `S` hides
  exactly the state that explains a machine that feels hung, since a process
  blocked on disk cannot be signalled away.

  `comm_buf` truncated to empty. `ArrayString::try_push_str` rejects a string that
  does not fit and leaves the buffer as it was, so a name over 64 bytes produced a
  blank COMMAND rather than a shortened one. The test's own fully qualified path
  triggered it every time.

  `ELAPSED` subtracted the boot time twice: `uptime - (start - boot)` is 0 for every
  process on a machine whose clock is newer than its boot, which is all of them.

  `cwd` was None for every process. `KERN_PROC_CWD` returns a `struct kinfo_file`,
  not a path, and a `PATH_MAX` buffer is too small for it (`ENOMEM`); reading the
  buffer as a string yields a NUL byte, which is indistinguishable from a permission
  refusal.

  `KERN_PROC_FILEDESC`'s records are variable-length. `libc`'s `kinfo_file` is 1392
  bytes; the kernel truncates each record after `kf_path` and reports 376-408.
  Guarding on `stride < size_of` — the natural check — rejects every record the
  kernel sends, so `l` reported "cannot read file descriptors" for processes whose
  descriptors were fine. The stride is the record's own `kf_structsize` and
  `kf_path` sits at a fixed offset of 368, because everything past the truncation
  point was never copied out.

Three more that came from comparing against the system's own tools

  `hw.physmem` exceeds `v_page_count * pagesize` by the kernel's early allocations,
  so a bar scaled to it can never fill. `kvm_getswapinfo` reports pages, not bytes
  (htop's FreeBSD backend multiplies by 1024 and labels the result bytes).
  `devstat`'s `pass(4)` devices double-count every byte next to the `da`/`ada`
  device they front.

The rest is wiring

  A third `Sampler` alias, `capability::PLATFORM`/`HOST_DISK_IO`/`AFFINITY` arms,
  `cpuset_*` for the affinity panel (real read/write, unlike macOS's scheduler
  hint), `devname_r` for TTY, and `e`/`l` mapped onto
  `KERN_PROC_ENV`/`KERN_PROC_FILEDESC` instead of `/proc`. `x` says why FreeBSD has
  no per-process lock table.

  Memory is its own model again: the five page queues partition physical memory, so
  `used = active + wired + laundry`, `cache = inactive + page cache`, and the
  segments can be checked against `v_page_count`. `laundry` counts as used for the
  same reason macOS's compressor does — those pages cannot be reused until the
  write completes.

CI

  A cross-compile check (cheap, catches a forgotten `cfg` arm) and a `vmactions` VM
  job running the real suite, because "it compiles" is worth little here. Releases
  build FreeBSD inside the VM: cross-linking wants FreeBSD's `ld` and `-lkvm`, and
  faking it with a sysroot produces a binary that links and then fails at run time.

Also in this release

  The post-M3 audit's nine restored identity columns. `Session`, `Tty`, `Tpgid`,
  `StartTime`, `Tgid`, `SchedPolicy`, `ProcComm`, `ProcExe` and `Cwd` were declared
  in `Field` with no `FieldMeta`, so `Field::name()` returned "UNKNOWN", the htoprc
  importer silently dropped them, `sort_key=` fell through to PERCENT_CPU, and they
  never appeared in the setup screen. `TTY` and `TPGID` were also never parsed, so
  even with metadata they would have rendered a constant. `s` gained a screen that
  explains why it is not implemented, instead of doing nothing at all.

M4 was already on main, untagged

  This tag is the first that can build FreeBSD binaries, which is what the version
  number is for.

Tests

  374 on macOS, 363 in the FreeBSD guest, zero clippy warnings on both.

Two bugs this tag itself exposed

  A test asserted `devname(0x10000000) == "ttys000"`. `devname(3)` answers from the
  live `/dev` table, so that is true on a Mac with open terminals and false on every
  CI runner, which has none. The renderer was right in both cases — it falls back to
  the raw number — so the test was asserting a property of the host rather than of
  the code. It now asks `devname` what the answer should be and compares, so it
  checks agreement with the platform instead of agreement with one machine.

  The FreeBSD archive was never attached to a release. `release` declared
  `needs: build`, and `freebsd` is a sibling job, so `download-artifact` could not
  see its artifact: the job built the tarball, uploaded it, and the file expired
  unused while the release body listed `x86_64-unknown-freebsd` in its platform
  table. Every release so far was missing it and nothing was red. It took FreeBSD
  being the headline feature for the absence to be the first thing anyone looked
  for, which is the risk of a job that succeeds at doing nothing.

  Both are fixed in this tag's history, and the CI run that gated it is green on
  all five jobs including the FreeBSD VM.

<sub>[compare v0.5.0...v0.6.0](https://github.com/pandasir/gander/compare/v0.5.0...v0.6.0)</sub>

## v0.5.0 — 2026-10-19

**M3 complete — delay accounting**

The last of M3's five items. `CPUD%`, `IOD%` and `SWPD%` report the share of a
task's life spent *waiting* rather than working: runnable but not on a CPU, blocked
on synchronous block I/O, and waiting for a page to be swapped back in.

Why it is worth a column

  CPU% says how much work a process does. It cannot say how much of its time was
  spent unable to do any. A process at 0.0 % CPU with 40 % `IOD%` is not idle, it is
  stuck, and every other column in the table shows the same thing for a process
  that is genuinely doing nothing. That distinction is the whole point.

Raw netlink, no libnl3

  htop reaches this data through `libnl3`, `dlopen`ed and called through function
  pointers so a missing library degrades instead of failing. The protocol is four
  messages — resolve the family, send a pid, parse a nested attribute, done — so
  gander speaks it directly. The crate's dependency list is still `libc` and
  nothing else, and there is no library to be missing.

Two ways to be absent, one rendering

  `TASKSTATS_CMD_GET` requires `CAP_NET_ADMIN`. An ordinary process does not have
  it, so unprivileged `gander` gets `EPERM` on every request: the normal case, not a
  fault. The socket records the first refusal and stops asking — one wasted syscall
  rather than one per process per tick — and every row renders `-`.

  `CONFIG_TASKSTATS=n` is the other: the generic-netlink family does not resolve and
  `DelayAcct::open()` returns `None`. Both absences look the same from the table,
  which is correct, because the user cannot act on the difference.

  `-` is not `0.0%`. `0.0%` claims the process never waited; `-` says nobody
  measured. Same rule the PSS columns follow, for the same reason.

A lifetime average, and why that has to be said out loud

  The kernel keeps cumulative `*_delay_total` counters in nanoseconds and
  `ac_etime` in microseconds, so `delay / (etime × 1000)` is the share of the task's
  entire life spent waiting. It does not react to a change within one tick: a stall
  an hour ago still shows for the next hour.

  That is a property of the data, not a choice, and it makes the number easy to
  misread as "waiting now". The column description says "share of this task's life",
  the README says it next to the table, and ROADMAP says it in full.

  `cpu_delay_total` is documented as updated non-atomically against `ac_etime`, so a
  sample can show more delay than elapsed time. Clamped to 100: the honest reading
  is "essentially all of it", and an unclamped value would sort above a process that
  really is stalled all the time.

The bug worth recording

  `TASKSTATS_TYPE_AGGR_PID` was `1` in the first version — the attribute enum
  counted from `PID` instead of from `UNSPEC`. `STATS = 3` happened to be right,
  which is why the parser looked plausible and returned `None` on every real kernel
  while all nine unit tests passed. The tests fed synthetic blobs straight to
  `parse_taskstats` and never exercised the attribute walk at all.

  The real order is `UNSPEC=0, PID=1, TGID=2, STATS=3, AGGR_PID=4, AGGR_TGID=5,
  NULL=6`, and a thread-group leader is answered with `AGGR_TGID` — a shape the
  first version did not handle in any form. Found by running against a real kernel
  with `CAP_NET_ADMIN` and printing the attribute tree. The module now carries the
  enum order in a comment, and the live tests assert all three columns arrive
  together or not at all, which is what a broken attribute walk looks like from
  outside.

Also in this release

  `--sort` on an unknown field exits 2 with a reason instead of silently sorting by
  pid, and a Linux-only column named on another platform says so specifically
  rather than being reported as a typo. That was the v0.4.0-era bug fixed in this
  cycle; it shipped in 0.4.0's tag but the fix is included here for anyone
  comparing.

Tests

  356 on macOS, 366 on Linux, zero clippy warnings on both. The delay path is
  verified three ways: unit tests on the parser and the attribute walk, live tests
  against a real kernel both with `CAP_NET_ADMIN` (data arrives) and without it
  (`EPERM` once, then dashes), and a gated-off check that the columns cost nothing
  when they are not on screen.

<sub>[compare v0.4.0...v0.5.0](https://github.com/pandasir/gander/compare/v0.4.0...v0.5.0)</sub>

## v0.4.0 — 2026-10-19

**M3, Linux depth**

Four features that read Linux kernel files with no Darwin equivalent, and one
correction that came out of building them.

What shipped

  PSS / EPSS / M_PRIVATE / M_SWAP columns from `/proc/<pid>/smaps_rollup`. RSS
  counts a shared page once per process, so two processes sharing 100 MB of
  libraries both report 100 MB and the RES column sums to 200 MB that does not
  exist. PSS divides each shared page by its number of sharers, which is the only
  per-process figure that adds up across a table.

  PSI pressure, as a meter and as a block in the `panels` system column. `some` is
  the share of wall time at least one task was stalled, `full` the share where
  nothing could run. This is the only figure that separates "busy" from
  "struggling": 90% CPU with an empty run queue is healthy, 30% CPU with `io full`
  at 40% is not, and CPU% and load average look the same in both cases.

  Zram / Zswap / HugePages meters. Each answers a question the memory bar cannot:
  how much swap is stored versus what it costs (4:1 means 4 GB occupying 1 GB),
  whether the next swapped page goes to disk, and how much memory is reserved and
  unusable by anything else.

  GPU% and GPU_TIME from the kernel's own `drm-usage-stats` keys in `fdinfo`. No
  vendor SDK, so `amdgpu`, `i915`/`xe` and `nouveau` work with nothing installed.

The cost gate

  `smaps_rollup` makes the kernel walk every VMA; `fdinfo` costs a directory walk
  and a read per descriptor. Either, across a desktop's table, costs more than the
  rest of the scan. Both are off unless a column that needs them is on screen, and
  the decision is derived from the active tab's visible columns — a PSS column on a
  tab nobody is looking at costs nothing.

  This replaced three `set_*` methods on the sampler trait with one `ScanOptions`
  value, packed into a single `AtomicU64` so the sampler thread can never observe a
  half-updated combination. With three setters, a forgotten call meant either a
  wasted scan or a column of dashes, and neither is visible from the outside.

Two things the code claimed that a real kernel disproved

  `/proc/pressure/cpu` *does* have a `full` line. An early version asserted it could
  not, on the reasoning that a CPU stall is per-task; a live test failed
  immediately. The parser had always read whatever the file contained, so only the
  comments and one test were wrong — and that test would have passed on the fixture
  and failed on every real machine.

  The htop field ids for the PSS block are 119/120/121/131/132/133/135, from
  `linux/ProcessField.h`. An earlier draft guessed 91..94. The importer matches on
  `id`, not on the `id - 1` the file stores, and getting that backwards is what made
  the first version of the test assert the wrong mapping.

--sort no longer lies

  `--sort BOGUS` and `--sort M_PSS` on macOS produced byte-identical output: a
  pid-ascending list with no indication that the requested order was not applied.
  The `unwrap_or(PercentCpu)` fallback never ran — an unresolvable field reached
  `compare_by`'s `_` arm, which compares pids. An unknown name is now an error with
  two messages, because a column that exists on another platform is a platform
  limit and anything else is a typo.

  Found by re-running the M3 build and looking at the output rather than the test
  result. It is the same failure mode the cost gate exists to prevent, on the side
  where a feature is requested and cannot be delivered.

Not done

  Delay accounting (`TASKSTATS` netlink), the fifth M3 item, is not implemented and
  is the only unchecked box in the milestone.

Tests

  350 on macOS, 349 on Linux, zero clippy warnings on both. The Linux figure comes
  from `rust:1.89-slim` in Docker, including `linux_live.rs`, which compares the new
  reads against the same `/proc` files parsed independently in the same test.

<sub>[compare v0.3.1...v0.4.0](https://github.com/pandasir/gander/compare/v0.3.1...v0.4.0)</sub>

## v0.3.1 — 2026-10-19

**the macOS memory figure matches htop again**

A user ran `gander` next to `htop` on a 16 GB Mac and got 5.3G from one and 13.3G
from the other. Neither tool was misreading anything: gander was faithfully
reproducing htop **3.3.0**, whose macOS compressor assignment is commented out,
while the htop on that machine was 3.4.1, which counts it. Reproducing an old
release is not a feature anyone asked for, and the result was a meter that
disagreed with htop, `top` and Activity Monitor by the size of the compressor —
the largest single term on a machine that has been up for a day.

What changed

  `used = (internal - purgeable) + wired + compressed`

  That is what htop 3.4.1 prints as `USED + COMPRESSED`. htop `main` (3.6-dev)
  states the same figure directly as a class list, so one formula covers both,
  and it matches Activity Monitor. Building the figure from the by-usage classes
  instead (`active + inactive + speculative + wired + compressed - purgeable -
  external`) is algebraically the same but double-counts `inactive` if any term
  is dropped, which is exactly how the abandoned `finish_darwin_36` came to
  understate memory by 1.6G here.

The bar was lying too

  The `free` segment was derived as `total - used - cache`. Once `used` stopped
  counting the compressor, the compressor fell into that segment: the bar drew
  7.8G free on a machine with 0.3G actually free, on the same screen that showed
  3.75G of swap in use. `free` now comes from the kernel's own free count and the
  remainder goes to a `cache` segment, so the segments partition physical memory
  exactly — `wired + active + compressed + purgeable + cache + free == total`,
  asserted on synthetic and live input alike.

Linux is untouched: `used = total - free - cached - sreclaimable - buffers`
already matched `linux/LinuxMachine.c` verbatim.

Tests

  `darwin_used_matches_htop_330` pinned the 3.3.0 number in place; it is replaced
  by `darwin_used_matches_htop_341`, built from the `vm_stat` sample taken at the
  moment the two tools disagreed, plus `darwin_free_segment_is_not_the_compressor`
  and `darwin_bar_segments_partition_total`. `tests/mem_accounting.rs` now asserts
  that `used` includes the compressor, that the segments sum to `total`, and that
  the free segment never exceeds the kernel's free count — the last two are what
  would have caught the bar drawing the compressor as free.

Patch rather than 0.4.0: no default changed and no interface moved, but a figure
every user reads at a glance was wrong by more than half of physical memory.

Version bumped to 0.3.1 so `gander --version` and the release archive names
match; the install snippets in both READMEs follow.

<sub>[compare v0.3.0...v0.3.1](https://github.com/pandasir/gander/compare/v0.3.0...v0.3.1)</sub>

## v0.3.0 — 2026-10-19

**M2 complete**

Stable, readable list

  The process list opens sorted by PID, ascending. htop opens by CPU% descending,
  which reorders on every sample — measured here, 10 of 12 visible rows changed
  position within 1.5 s, so the row you were looking at moved out from under the
  cursor before you could act on it. PID order holds for as long as a process
  lives. F6 / < > still reaches CPU% in one key, and htop's behaviour is one config
  line away.

  Sorting is deterministic: ties break by PID ascending in both directions, so an
  equal group never shuffles. Two bugs were behind the instability — the descending
  path reversed every tie as well as the primary key (a quiet machine listed pids
  backwards), and `sort_unstable_by` left ties unordered. `App::new` also
  overwrote the configured sort direction unconditionally, which made
  `desc = false` ineffective for every field.

M2's remaining items

  Bulk renice: F7/F8 now act on the tagged set, like F9 already did.
  Base-name highlighting and replaced/deleted executable colouring: both options
  existed and neither was read by anything.
  O hides containerised processes (Linux; says why elsewhere).

Fixes found along the way

  - The status line was never drawn. A dozen actions wrote App::status — renice
    results, kill failures, the reason O does nothing on macOS — and no code
    rendered it, so every message was invisible. It is now painted on the function
    bar and cleared by the next action that has nothing to say.
  - `derive(Default)` on Visibility and ProcTable would have hidden kernel threads,
    because their toggles mean "hidden" when false. Both are written out by hand.
  - htoprc import ignored `hide_userland_threads`.
  - O and F7/F8 and the base-name work are covered by end-to-end checks: a real
    nested PID namespace for the container filter, and captured cell colours for
    the highlight.

322 tests on macOS, 303 on Linux, zero clippy warnings on both.

<sub>[compare v0.2.0...v0.3.0](https://github.com/pandasir/gander/compare/v0.2.0...v0.3.0)</sub>

## v0.2.0 — 2026-10-19

**M1 complete, plus container columns**

Re-release. The first v0.2.0 was tagged one push too early: four commits landed
immediately after it, and two of them matter for the artefacts.

  - `cargo clippy --all-targets -- -D warnings` failed on clippy 1.99, and
    `rust-version` claimed 1.75 while ratatui 0.30 needs 1.88 — so the stated
    build path was broken twice over.
  - The README described keys that do not exist as described and quoted a
    measurement from a different machine. Both archives ship the README.

The tag now points at the tree that actually passes CI. No code behaviour changed
relative to the previous tag except the two lint fixes; the source archives and
the binaries are simply built from a tree that builds.

M1 — feature parity for daily use

  NetworkIO and DiskIO meters. Both are rates over cumulative kernel counters,
  scaled against the largest rate observed so far rather than a fixed ceiling: a
  bar pinned to 1 Gbit is an invisible sliver on a 100 Mbit link, and one pinned
  to 100 Mbit sits saturated on the faster one.

    Linux network   /proc/net/dev, loopback excluded
    Linux disk      /proc/diskstats, whole disks only via /sys/block
    macOS network   sysctl(NET_RT_IFLIST2), because getifaddrs counters are 32-bit
    macOS disk      no host-wide counter without IOKit; the meter says so

  Search and filter history under $XDG_STATE_HOME, recalled with Up/Down.

M2 — container columns

  CGROUP (the path) and CONTAINER (runtime:id) on Linux. Detection uses the cgroup
  path for identity and the PID namespace for membership, because neither is
  sufficient alone: a container with its own cgroup namespace reads `/`, and the
  namespace says "containerised" without saying which.

Fixes

  - Esc could never clear a filter: the handler reset the input before reading its
    kind, which was by then None.
  - The search/filter prompt was never drawn, so a filter that hid every row
    looked like a sampler bug.
  - row.cgroup held the raw /proc line rather than the path.
  - container_from_cgroup returned a bare marker, so every container rendered the
    same word.
  - A Field with no arm in sort_order silently sorted by pid.
  - Two flaky macOS thread tests, both asserting something the kernel does not
    promise.
  - clippy manual_inspect and chunks_exact_to_as_chunks on the current stable.
  - rust-version now declares 1.89 and CI builds against it.

300 tests on macOS, 279 on Linux, zero clippy warnings on both.

<sub>[compare v0.1.1...v0.2.0](https://github.com/pandasir/gander/compare/v0.1.1...v0.2.0)</sub>

## v0.1.1 — 2026-10-19

**configurable per-core grid cap (system_cores_max)**

_The tag has no message body beyond its subject._

<sub>[compare v0.1.0...v0.1.1](https://github.com/pandasir/gander/compare/v0.1.0...v0.1.1)</sub>

## v0.1.0 — 2026-10-19

_Tagged without a message, so there are no notes for this one. It was the
first release; the notes start at v0.1.1._
