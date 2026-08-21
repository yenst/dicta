# Annotation overlay slice

`AnnotationOverlay.qml` is a separate, transparent full-screen window. It starts
in input pass-through mode, captures pointer/touch input only while annotation
mode is enabled, and returns to pass-through on Escape. `clearAndHide()` clears
both committed and in-progress scene-graph geometry before hiding.

`AnnotationItem` builds `QSGGeometryNode` line geometry on the Qt Quick render
thread. It intentionally does not use `QQuickPaintedItem` or a raster canvas.
The four functional tools are pen, arrow, rectangle, and spotlight.

`OverlayController` creates this window independently of `Main.qml` and exposes
the process boundary as the `dictaOverlay` context object. It can select an
exact `QScreen` by recording-output name, start the recording clock, switch
input modes and tools, undo, clear, finish/hide, and forward normalized strokes.
The next runtime adapter can consume those signals without putting recording
logic into QML.

## Wayland placement

The placement port reports `hyprland_bypass_toplevel`. It selects the exact
recording `QScreen` and requests a frameless, always-above bypass/tool
fullscreen transparent Qt surface. The controller remaps that same surface
when it switches between click-through and focused annotation input.
