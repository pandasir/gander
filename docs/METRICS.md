# Process Metrics and Batch Output

## Availability

A real zero is a measurement. A missing value is not zero. Core rows carry
field-specific availability alongside their raw scalar values. TUI and batch text
show `-` for unavailable resource values and do not paint a value bar. Sorting
places unknown values after measured values in either direction.

The exported `availability` map uses these statuses:

| Status | Meaning |
|---|---|
| `available` | The value was read or calculated over a valid interval, including genuine zero. |
| `warming_up` | A CPU interval has no usable baseline, including new identities, reset counters, or a topology reset. |
| `permission_denied` | The operating system denied the affected read. |
| `unsupported` | The platform cannot provide that measurement. |
| `read_error` | The read or its required denominator failed for another reason. |

Failures affect only the fields depending on that read. A readable PID, owner,
command name, or nice adjustment remains visible when task counters are denied.
On macOS, native task/thread scheduling priority is independent of nice: priority
31 and nice 0 are valid distinct values. An unreadable priority is not replaced
with nice. Linux and FreeBSD retain their native priority conventions.

JSON retains the existing top-level array and metric names. `cpu_percent`,
`mem_percent`, `rss`, `vsize`, `priority`, and `threads` are now nullable. Each has
a corresponding status in `availability`. Other identity fields remain unchanged;
the optional nested Linux `memory` object keeps its existing null semantics.

For example, a row whose memory read was denied can include:

```json
{
  "pid": 123,
  "rss": null,
  "mem_percent": null,
  "availability": {
    "rss": "permission_denied",
    "mem_percent": "permission_denied"
  }
}
```

This is a breaking value-contract change for consumers that assumed every basic
metric was a number. Do not replace null with zero when calculating averages.

## First Output and CPU Units

Batch execution takes an unreported baseline, waits one configured sampling
interval (clamped to 100-10000 ms), then emits its first measured snapshot.
`--json` still emits one array by default. `-n N` counts emitted arrays, not the
baseline; `-n 0` retains its historical one-output clamp. Repeated JSON snapshots
remain consecutive arrays, not NDJSON or a new envelope format.

```sh
gander --json -d 1000 --sort PERCENT_CPU --limit 5
gander --json -n 2 -d 1000 | jq -s '.'
gander --json --sort M_RESIDENT --limit 5 | jq '.[] | select(.rss != null)'
```

A task appearing after the baseline is included with unavailable CPU until it has
a valid identity-matched interval. CPU time accumulated before a first observation
is never presented as interval usage. PID reuse and counter regressions reset the
baseline. Numeric conversion retains safe internal zero values on invalid
intervals, but output surfaces consult availability before displaying them.

For valid intervals, ordinary task CPU still uses one occupied core as 100%; a
multi-core process can exceed 100%. `PERCENT_NORM_CPU` divides that value by the
sampled logical CPU count exactly once. Host CPU meters keep their machine-wide
fraction and are not rescaled by process CPU corrections.
