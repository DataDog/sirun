# Ready Signal Design

**Date:** 2026-06-03
**Status:** Approved

## Problem

Sirun starts measuring from the moment the benchmarked process is launched. For
interpreted-language apps (Node.js, Python, etc.), module loading and other
startup work can account for a significant portion of the measured time,
obscuring the actual benchmark signal.

## Goal

Allow the benchmarked app to signal when it is ready to be measured, so that
startup time is excluded from all reported metrics.

## Design

### Signal mechanism

Before spawning the benchmarked command, the inner sirun process (the one that
runs per-iteration, detected by `SIRUN_ITERATION`) creates a Unix pipe. The
write-end fd number is passed to the child via the `SIRUN_READY_FD` environment
variable. The write-end is explicitly inherited by the child using
`CommandExt::fd`; all other extra fds remain close-on-exec.

After spawning the child, the inner sirun closes its copy of the write-end and
runs an async `select!` racing two futures:

1. **Read from the pipe read-end** — data arrives, meaning the app signaled ready.
2. **Child exits** — the process ended before signaling.

### On signal received (case 1)

- `start_time` (`std::time::Instant`) is reset to now.
- On Linux with `instructions: true`: the counter is stopped, its value saved
  as `startup_instructions`, then restarted. The final post-ready instruction
  count is `total_instructions - startup_instructions`. (Stop/restart is used
  rather than reading while running, since `perfcnt` requires the counter to
  be stopped before reading.)
- Only the first write to `SIRUN_READY_FD` has any effect. After the first
  read, Sirun stops watching the pipe and proceeds to wait for the child to exit.

### On child exit before signal (case 2)

Silent fallback: Sirun uses the original `start_time` and `rusage` baseline.
No warning is emitted. The app simply did not use the feature.

### Usage

```js
// Node.js — after all imports and startup work
require('fs').writeSync(parseInt(process.env.SIRUN_READY_FD), 'x');
```

```python
# Python — after all imports and startup work
import os
os.write(int(os.environ['SIRUN_READY_FD']), b'x')
```

```sh
# Shell — after startup work
echo x >&$SIRUN_READY_FD
```

## Metrics

| Metric              | When signal received          | When no signal      |
|---------------------|-------------------------------|---------------------|
| `wall.time`         | Post-ready period only        | Full lifetime       |
| `user.time`         | Full lifetime (RUSAGE_CHILDREN limitation) | Full lifetime |
| `system.time`       | Full lifetime (RUSAGE_CHILDREN limitation) | Full lifetime |
| `cpu.pct.wall.time` | Full lifetime (derived from user+system)   | Full lifetime |
| `instructions`      | Post-ready only (Linux)       | Full lifetime       |
| `max.res.size`      | Full lifetime (not reset)     | Full lifetime       |

`max.res.size`, `user.time`, `system.time`, and `cpu.pct.wall.time` always
reflect the full process lifetime. `RUSAGE_CHILDREN` (used for the rusage
metrics) only updates after a child exits and is waited for, so it cannot be
sampled mid-run in a meaningful way.

## Config changes

No new config fields. The pipe is always created and `SIRUN_READY_FD` is always
set in the child's environment. Apps opt in by writing to the fd.

## Implementation notes

- Pipe creation and the `select!` live in `run_test()` in `src/main.rs`.
- `run_cmd()` in `src/subproc.rs` needs a new optional parameter (or a
  separate variant) to accept an fd to explicitly inherit via
  `std::os::unix::process::CommandExt::fd`.
- The Linux instruction counter path in `run_with_instruction_count()` needs
  to accept a signal channel; on signal it stops the counter, saves the value,
  and restarts it. The final reported value is `total - startup_instructions`.
- `SIRUN_READY_FD` should be documented in the README alongside the other
  environment variables.
