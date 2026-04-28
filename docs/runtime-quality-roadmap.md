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

## FramePose design notes

Current frame boundaries are split across `Puppet` methods:

- `begin_frame()` resets render deforms, relative transforms/z-sort, and parameter context to model defaults.
- Host code then writes base parameter input through `ParamCtx::set()`.
- `end_frame(dt)` applies base parameters, applies `physics_input_offsets` to relative transforms, updates absolute transforms, steps physics, resets render/transform state again, writes physics output parameters, reapplies parameters, updates transforms, and finally updates render buffers/z-sort order.
- `apply_post_physics_param_overrides()` is a separate post-`end_frame` entry point. It writes parameter values, resets render/transform state, reapplies the whole parameter context, and rebuilds transforms/render buffers.
- `apply_post_physics_transform_offsets_by_names()` mutates final visible relative transforms after physics, then updates transforms. It currently does not rebuild render buffers.

`FramePose` should make those phases explicit and keep the following state separated until commit:

- base parameter input: values requested by the host before physics;
- physics input transform offsets: node-local offsets that affect only physics sampling;
- physics output: parameter values produced by `PhysicsCtx::step()`;
- post-physics parameter overrides: values that replace, add to, or clamp physics/base output for presentation;
- post-physics visible transform offsets: node-local offsets that affect only the final rendered pose;
- diagnostics: missing handles, skipped nodes, and override conflicts collected during commit.

The first Rust patch should keep this API internal and connect it to existing `Puppet` phases without changing public behavior:

- introduce resolved `FrameParamHandle` and `FrameNodeHandle` wrappers around existing UUID/name resolution;
- let `Puppet::begin_frame()` create or reset a `FramePose`;
- move `physics_input_offsets` from direct `Puppet` state into the pose transaction;
- have `Puppet::end_frame()` delegate phase ordering to the pose transaction;
- keep the current name-based helper methods as compatibility wrappers that write into the current pose.

## Snapshot fields for host apps

A Live2D-like host app needs a renderer-independent snapshot after commit. The minimal useful shape is:

- frame phase/result: committed frame id, `dt`, whether physics ran, and diagnostic list;
- parameter values by resolved handle/name: base input, physics output, post-physics override, and final value;
- node transforms: UUID, name, enabled flag, parent UUID, relative transform, absolute transform matrix, inherited z-sort, and whether post-physics offsets were applied;
- draw order: root drawable UUIDs in final z-sort order and composite child drawable UUIDs in final z-sort order;
- drawable buffer ranges: for textured meshes, vertex range, index range, deform range, texture id, blend mode, and mask/composite relation;
- active deforms: target node UUID, source kind (`Param` or `Node`), source id, enabled flag, and affected vertex count;
- skipped nodes: UUID, name, phase (`InitRender`, `UpdateRender`, `CommitPose`), and reason such as disabled, non-renderable, missing mesh, missing deform stack, or composite child excluded from root draw list.

These fields let tests and host debug panels assert final pose/render state without requiring an OpenGL/WebGL backend.

## 2026-04-28 resolved node handles

Added a small public node-handle API as the first step toward typed runtime
effects:

- `Puppet::resolve_node_by_name()`
- `Puppet::resolve_nodes_by_names()`
- `Puppet::set_physics_input_offset_by_handle()`
- `Puppet::apply_post_physics_transform_offsets_by_handles()`
- `Puppet::set_drawable_opacity_by_handles()`

This keeps the existing name-based helpers for compatibility, while allowing
host integrations to resolve motion targets once after model load and reuse
opaque handles during frame playback. The immediate use cases are Live2D-like
`PartOpacity`, post-physics visible offsets, and future gesture/secondary-motion
effects without repeated name lookup in the hot path.

## 2026-04-28 resolved parameter handles

Added the matching parameter-handle API:

- `Puppet::resolve_param_by_name()`
- `Puppet::set_parameter_by_handle()`
- `Puppet::apply_post_physics_param_overrides_by_handles()`

This lets host integrations resolve authored parameter names once after model
load, then drive base motion values and post-physics presentation overrides via
typed handles. Name-based parameter APIs remain as compatibility wrappers, but
new Live2D-style retargeting code should prefer resolved handles so missing or
renamed parameters fail at load/retarget setup time instead of being rediscovered
inside every frame.

## 2026-04-28 frame context and snapshot foundation

Added the first runtime-owned frame inspection surface:

- `Puppet::frame_context()` exposes the current frame id, `dt`, physics state,
  and the pose inputs/effects recorded through resolved handle APIs.
- `Puppet::snapshot_frame()` returns renderer-independent node transforms, draw
  order handles, drawable buffer ranges, composite child order, and skipped
  disabled nodes.
- `RenderCtx::root_drawables_zsorted()` exposes draw order to the snapshot API
  without requiring a renderer backend.

This does not yet replace the existing `begin_frame()` / `end_frame()` flow, but
it makes the current phase ordering observable and testable. The next step is to
move runtime effect storage out of host-side sequencing and into an explicit
pose transaction that commits the whole frame in one call.
