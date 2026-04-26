# Runtime quality roadmap

This note tracks runtime-side work that should replace app-side ad hoc correction when aiming for Live2D-like stability.

## Current failure class

Mouth, nose, and eye artifacts usually come from frame-phase ambiguity:

- physics output and presentation overrides are applied in different spaces;
- render buffers can be rebuilt from already-mutated transform/deform state;
- disabled or non-renderable nodes leak into render/deform traversal;
- degenerate parameter ranges produce unstable interpolation at axis boundaries.

The runtime should make the valid frame order explicit and testable instead of requiring host apps to patch mouth or face nodes after rendering.

## Runtime API priorities

1. Add a `FramePose` or `FrameContext` API that separates base parameter input, physics input offsets, physics output, post-physics parameter overrides, and final visible transform offsets.
2. Expose named override targets as resolved node/parameter handles. Host apps should resolve names once, then apply typed handles each frame.
3. Provide a post-transform/render snapshot API for tests and debug tooling. It should expose final relative/absolute transforms, z-sort order, active deforms, render-buffer ranges, and skipped-node reasons without requiring a backend renderer.
4. Make override conflict policy explicit. Parameter overrides should declare whether they replace physics output, add to it, or only clamp it.
5. Keep disabled/non-renderable nodes observable as skipped, not silently mixed into draw lists or deform stacks.

## Regression test matrix

- Disabled nodes: enabled mesh siblings render, disabled mesh siblings do not allocate render contexts or vertex-buffer ranges.
- Degenerate interpolation: zero-width or zero-height parameter cells return the beginning value and never produce NaN.
- Parameter overrides: post-physics overrides rebuild transforms/deforms from reset state so values do not accumulate across the same frame.
- Deform targets: missing mesh/deform-stack targets are skipped with diagnostics instead of panicking during normal playback.
- Render context: composite children and root drawables keep deterministic z-sort after physics and overrides.

## Near-term implementation target

The next runtime API should be a small pose transaction:

```rust
puppet.begin_frame();
pose.set_param(param, value);
pose.set_physics_input_offset(node, offset);
puppet.solve_physics(dt);
pose.override_param_after_physics(param, value, OverrideMode::Replace);
pose.offset_transform_after_physics(node, offset);
let snapshot = puppet.commit_pose_and_snapshot();
```

The important behavior is not the exact naming, but that the runtime owns the order and exposes a snapshot that tests can assert against. That gives aituber-studio a stable integration point without model-specific mouth, nose, or eye corrections.
