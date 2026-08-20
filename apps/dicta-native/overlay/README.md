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

## Wayland placement gate

The current placement port reports `best_effort_qt_toplevel` and
`guaranteedLayerShell == false`. It selects the correct `QScreen` and requests a
fullscreen transparent Qt toplevel, but does not claim overlay-layer ordering.
`WindowStaysOnTopHint` was deliberately removed because it cannot guarantee
wlroots layer-shell semantics.

Guaranteed placement requires a Qt-compatible layer-shell client integration
that can attach Qt's existing `wl_surface` to `zwlr_layer_shell_v1`, select the
overlay layer, anchor all four edges, use a non-exclusive zone, configure
keyboard interactivity, and bind the recording output's `wl_output` before the
first commit. This system has Wayland client headers but no LayerShellQt-style
dependency, so that implementation remains isolated behind
`OverlayPlacementPort` instead of being faked in QML.
