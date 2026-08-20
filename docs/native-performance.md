# Native performance and bloat baseline

This is a reproducible baseline for the native Qt shell, not a product-launch
claim. Run [`scripts/benchmark-native.sh`](../scripts/benchmark-native.sh) to
regenerate a Markdown report from the artifacts already on the machine:

```sh
scripts/benchmark-native.sh
```

The script never invokes a package manager, downloads dependencies, drops Linux
page caches, starts the existing Tauri application, or strips/replaces an
artifact. Use `--build-native` when an explicit Release rebuild is wanted. Use
`--runtime gui` from an interactive Hyprland terminal for startup-to-window-map
and compositor-backed RSS measurements.

## Baseline from 2026-08-20

Environment: Arch Linux, kernel 7.1.8, x86-64, Qt 6.11.1. The native artifact
was the CMake Release executable at `apps/dicta-native/build/dicta-native`. The
comparison artifact was the already-present `src-tauri/target/release/dicta`;
the benchmark did not rebuild or launch it.

| Measurement | Native Qt | Existing Tauri | Interpretation |
| --- | ---: | ---: | --- |
| ELF file size | 5,303,904 B (5.1 MiB) | 57,913,880 B (56 MiB) | Native is 90.8% smaller; Tauri is 10.92× larger |
| Direct `DT_NEEDED` entries | 13 | 17 | Exact direct ELF dependencies |
| Resolved `ldd` lines | 77 | 145 | Host-specific transitive runtime view |
| Missing shared libraries | 0 | 0 | Both current artifacts resolve on this host |
| Idle RSS | 61,148 KiB | Not measured | Native median of five samples after one second, using Qt offscreen |
| Startup to event loop/window map | Not measured | Not measured | Hyprland's socket was inaccessible from the agent sandbox |

The size and ELF dependency measurements are reliable for these exact files.
The `ldd` count is host-specific and is not an installed-size total: shared
libraries may already be resident and are shared between processes. The RSS
number is an offscreen smoke baseline, not a compositor-backed desktop number.

Both measured executables were unstripped, so the table compares like with like
but does not predict final package size. Production packaging should repeat the
comparison using the actual shipped artifacts.

## Startup methodology

In GUI mode the benchmark records the time immediately before `exec` and polls
Hyprland until a client with the exact child PID appears. That mapped-window
observation is used as a practical event-loop/render-path proxy. The first run
is reported separately and the remaining runs produce a warm median.

The first run is deliberately called “first,” not “cold.” A true cold-cache run
would require controlling or dropping OS caches, which is privileged,
disruptive, and outside this benchmark. Results from different machines should
include CPU, storage, compositor, Qt, and kernel versions.

Recommended interactive run:

```sh
scripts/benchmark-native.sh --build-native --runtime gui --runs 11 --idle-seconds 5 \
  > /tmp/dicta-native-benchmark.md
```

If Hyprland cannot be queried, `auto` falls back to Qt offscreen mode. That mode
still samples native idle RSS but prints startup timing as “not measured” because
there is no honest event-loop-ready marker in the current application.

## Dependency shape

The native executable directly links the Qt Core, GUI, QML, Quick, OpenGL, and
Network libraries plus the normal C/C++ runtime and GL loader. The existing
Tauri executable directly links GTK 3, WebKitGTK/JavaScriptCore, Soup, GLib/GIO,
Cairo, Vulkan, and the normal runtime libraries. Exact names and SHA-256 hashes
are emitted on every benchmark run so results can be tied to an artifact.

## Process cleanup

Runtime probes retain only the PID returned by starting that exact benchmark
child. Normal completion and `EXIT`, `INT`, or `TERM` traps send `SIGTERM` to
that PID and wait for it to be reaped. The script does not use `pkill`, process
name matching, or broad cleanup patterns. Its one temporary diagnostic log is
removed explicitly and its private temporary directory is then removed.

## Remaining measurements before release

- Repeat GUI startup and RSS on the target Omarchy/Hyprland session outside a
  sandbox, with no other Dicta instance running.
- Run enough samples to report median and p95; preserve the raw report and exact
  artifact hashes in release CI.
- Compare installed package size, not just ELF size, once QML modules and
  packaging contents are finalized.
- Measure idle CPU wakeups and recording CPU/GPU load after the capture pipeline
  is connected; the current shell baseline cannot represent recording cost.
- Compare the final stripped production binaries built from the same revision.
