# Security Policy

## Supported versions

Security fixes are made on the latest release. At the time of writing that is
the `0.6.x` series.

| Version | Supported |
|---|---|
| `0.6.x` | Yes |
| `< 0.6` | No |

## Reporting a vulnerability

Do not open a public issue, discussion, or pull request for a vulnerability.

Use GitHub's private vulnerability reporting:

1. Open the repository's **Security** tab.
2. Choose **Report a vulnerability**.
3. Include the affected version and platform, the impact, and the smallest
   reproduction you can provide.

If the private reporting button is unavailable, open a public issue containing
only a request for a private contact channel. Do not include exploit details,
process data, paths, tokens, or personal information in that issue.

Please include:

- the gander version from `gander --version`;
- the operating system and kernel version;
- how gander was installed;
- whether the issue requires elevated privileges or a specific terminal;
- the impact and the steps that demonstrate it;
- a suggested fix or mitigation, if you have one.

Remove secrets and unrelated personal data from logs and screenshots. A process
list can reveal usernames, paths, hostnames, and command-line arguments.

## Scope

Reports are especially useful for:

- privilege boundaries around signals, renice, affinity, or I/O priority;
- terminal escape-sequence injection from process names or command lines;
- unsafe parsing of `/proc`, `sysctl`, Mach, or `libkvm` data;
- path traversal or unsafe writes in configuration and history handling;
- container or PID-namespace misidentification;
- a crash or denial of service caused by malformed kernel data.

General bugs, inaccurate readings, and feature requests belong in the normal
issue tracker.

## Disclosure

Please allow time for a fix and a release before public disclosure. Maintainers
will coordinate the timing with the reporter and credit the reporter unless
they ask to remain anonymous. There is no bug bounty.
