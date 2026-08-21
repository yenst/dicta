# Dicta context for Omarchy

This bar plugin keeps Dicta's quick project/context workflow inside the
Omarchy shell:

- Search every linked Dicta project while keeping at most three rows visible.
- Click a project to select it and open Dicta on that project.
- Switching projects keeps the panel open; Escape or clicking outside closes it.
- Browse the three newest recordings for the selected project.
- Click a context row to copy the same recording-context prompt used by Dicta.
- Project-open and context-copy icons appear only on pointer hover or keyboard
  focus; the whole row remains the target.
- Use Space or Enter for the row's quick action. Right Arrow opens Dicta after
  selecting the focused project, or opens the general Dicta UI for a recording.
  Copy success is shown inline.
- Open Dicta from the panel header. The bar stays to one Dicta icon; left click
  opens this panel, middle click refreshes it, and right click opens Dicta.

The QML plugin is intentionally thin. It talks only to the native `dicta` CLI:
passive refreshes use `--no-start --json` project/recording reads, project rows
run typed `project select` before `dicta ui`, and context copy uses `context ...
--copy`. Every command is an argv list;
the plugin has no shell pipeline, MCP helper, service, port, or separate store.

Recording rows use the persisted note (then transcript preview or ID) as their
display title and show relative time from the typed recording summary. Right
Arrow opens the exact recording through `dicta recording open ID`; the running
native host selects the detail before raising its existing window.

## Install from this checkout

```bash
./integrations/omarchy/install.sh
```

The installer validates the manifest, copies the plugin to
`~/.config/omarchy/plugins/dicta.context/`, and enables it in the bar. Omarchy
hot-reloads the shell configuration. It refuses to overwrite an existing
plugin directory.

For a development build of the CLI, point the plugin at it after enabling:

```bash
omarchy bar set dicta.context dictaCommand "$PWD/target/debug/dicta"
```

The installed CLI and native host must be from the same Dicta release so their
typed commands and local socket protocol agree.
