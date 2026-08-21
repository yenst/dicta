# Native performance baseline

Run [`scripts/benchmark-native.sh`](../scripts/benchmark-native.sh) to generate
a Markdown report for the current Qt host:

```sh
scripts/benchmark-native.sh
```

The script records the exact ELF size and hash, direct and resolved library
counts, and an idle RSS sample. From an interactive Hyprland terminal,
`--runtime gui` also measures process start to the compositor's mapped-window
event. `--build-native` performs an explicit Release rebuild first.

The measurement does not download dependencies, invoke a package manager, drop
Linux page caches, strip the binary, or use broad process-name cleanup. The
first GUI run is therefore labelled “first,” not “cold”; later runs produce a
warm median.

Recommended release run:

```sh
scripts/benchmark-native.sh --build-native --runtime gui --runs 11 \
  --idle-seconds 5 > /tmp/dicta-native-benchmark.md
```

Reports should preserve the CPU, storage, compositor, Qt, kernel, artifact
hash, median and p95. Recording CPU/GPU load should be measured separately from
idle shell cost because it depends on the selected encoder and output.
