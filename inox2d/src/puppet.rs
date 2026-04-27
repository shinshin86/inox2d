pub mod meta;
mod transforms;
mod tree;
mod world;

use std::collections::HashMap;

use glam::{Vec2, Vec3};

use crate::math::transform::TransformOffset;
use crate::node::components::{Drawable, TransformStore};
use crate::node::{InoxNode, InoxNodeUuid};
use crate::params::{Param, ParamCtx, SetParamError};
use crate::physics::{PhysicsCtx, PuppetPhysics};
use crate::render::RenderCtx;

use meta::PuppetMeta;
use transforms::TransformCtx;
pub use tree::InoxNodeTree;
pub use world::World;

/// Inochi2D puppet.
pub struct Puppet {
	pub meta: PuppetMeta,
	physics: PuppetPhysics,
	physics_ctx: Option<PhysicsCtx>,
	pub(crate) nodes: InoxNodeTree,
	pub(crate) node_comps: World,
	physics_input_offsets: HashMap<InoxNodeUuid, TransformOffset>,
	/// Currently only a marker for if transform/zsort components are initialized.
	pub(crate) transform_ctx: Option<TransformCtx>,
	/// Context for rendering this puppet. See `.init_rendering()`.
	pub render_ctx: Option<RenderCtx>,
	pub(crate) params: HashMap<String, Param>,
	/// Context for animating puppet with parameters. See `.init_params()`
	pub param_ctx: Option<ParamCtx>,
}

#[allow(dead_code)]
pub(crate) mod frame_api {
	use glam::{Mat4, Vec2};

	use crate::math::transform::TransformOffset;
	use crate::node::InoxNodeUuid;
	use crate::params::ParamUuid;

	#[derive(Debug, Clone, Copy, PartialEq, Eq)]
	pub(crate) enum FrameOverrideMode {
		Replace,
		Add,
		Clamp,
	}

	#[derive(Debug, Clone, Copy, PartialEq, Eq)]
	pub(crate) enum FrameSkipPhase {
		InitRender,
		UpdateRender,
		CommitPose,
	}

	#[derive(Debug, Clone, Copy, PartialEq, Eq)]
	pub(crate) enum FrameSkipReason {
		Disabled,
		NonRenderable,
		MissingMesh,
		MissingDeformStack,
		CompositeChildExcludedFromRootDrawList,
	}

	#[derive(Debug, Clone, PartialEq, Eq)]
	pub(crate) struct FrameParamHandle {
		pub(crate) uuid: ParamUuid,
		pub(crate) name: String,
	}

	#[derive(Clone, Copy, PartialEq, Eq)]
	pub(crate) struct FrameNodeHandle {
		pub(crate) uuid: InoxNodeUuid,
	}

	impl std::fmt::Debug for FrameNodeHandle {
		fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
			f.debug_tuple("FrameNodeHandle").field(&self.uuid.0).finish()
		}
	}

	#[derive(Debug, Clone)]
	pub(crate) struct FrameParamOverride {
		pub(crate) param: FrameParamHandle,
		pub(crate) value: Vec2,
		pub(crate) mode: FrameOverrideMode,
	}

	#[derive(Debug, Default, Clone)]
	pub(crate) struct FramePose {
		pub(crate) base_params: Vec<(FrameParamHandle, Vec2)>,
		pub(crate) physics_input_offsets: Vec<(FrameNodeHandle, TransformOffset)>,
		pub(crate) physics_outputs: Vec<(FrameParamHandle, Vec2)>,
		pub(crate) post_physics_param_overrides: Vec<FrameParamOverride>,
		pub(crate) post_physics_transform_offsets: Vec<(FrameNodeHandle, TransformOffset)>,
	}

	#[derive(Debug, Clone, PartialEq, Eq)]
	pub(crate) struct FrameDiagnostic {
		pub(crate) phase: FrameSkipPhase,
		pub(crate) node: Option<FrameNodeHandle>,
		pub(crate) message: String,
	}

	#[derive(Debug, Default, Clone)]
	pub(crate) struct FrameContext {
		pub(crate) frame_id: u64,
		pub(crate) dt: f32,
		pub(crate) physics_ran: bool,
		pub(crate) pose: FramePose,
		pub(crate) diagnostics: Vec<FrameDiagnostic>,
	}

	#[derive(Debug, Clone)]
	pub(crate) struct FrameNodeSnapshot {
		pub(crate) node: FrameNodeHandle,
		pub(crate) name: String,
		pub(crate) enabled: bool,
		pub(crate) parent: Option<FrameNodeHandle>,
		pub(crate) relative_transform: TransformOffset,
		pub(crate) absolute_transform: Mat4,
		pub(crate) zsort: f32,
		pub(crate) post_physics_offset_applied: bool,
	}

	#[derive(Debug, Clone, PartialEq, Eq)]
	pub(crate) struct FrameDrawableSnapshot {
		pub(crate) node: FrameNodeHandle,
		pub(crate) vertex_range: std::ops::Range<usize>,
		pub(crate) index_range: std::ops::Range<usize>,
		pub(crate) deform_range: std::ops::Range<usize>,
	}

	#[derive(Debug, Clone, PartialEq, Eq)]
	pub(crate) struct FrameSkippedNode {
		pub(crate) node: FrameNodeHandle,
		pub(crate) name: String,
		pub(crate) phase: FrameSkipPhase,
		pub(crate) reason: FrameSkipReason,
	}

	#[derive(Debug, Default, Clone)]
	pub(crate) struct FrameSnapshot {
		pub(crate) nodes: Vec<FrameNodeSnapshot>,
		pub(crate) root_drawables_zsorted: Vec<FrameNodeHandle>,
		pub(crate) composite_children_zsorted: Vec<(FrameNodeHandle, Vec<FrameNodeHandle>)>,
		pub(crate) drawables: Vec<FrameDrawableSnapshot>,
		pub(crate) skipped_nodes: Vec<FrameSkippedNode>,
	}
}

impl Puppet {
	pub(crate) fn new(
		meta: PuppetMeta,
		physics: PuppetPhysics,
		root: InoxNode,
		params: HashMap<String, Param>,
	) -> Self {
		Self {
			meta,
			physics,
			physics_ctx: None,
			nodes: InoxNodeTree::new_with_root(root),
			node_comps: World::new(),
			physics_input_offsets: HashMap::new(),
			transform_ctx: None,
			render_ctx: None,
			params,
			param_ctx: None,
		}
	}

	/// Create a copy of node transform/zsort for modification. Panicks on second call.
	pub fn init_transforms(&mut self) {
		if self.transform_ctx.is_some() {
			panic!("Puppet transforms already initialized.")
		}

		let transform_ctx = TransformCtx::new(self);
		self.transform_ctx = Some(transform_ctx);
	}

	/// Call this on a freshly loaded puppet if rendering is needed. Panicks:
	/// - if transforms are not initialized.
	/// - on second call.
	pub fn init_rendering(&mut self) {
		if self.transform_ctx.is_none() {
			panic!("Puppet rendering depends on initialized puppet transforms.");
		}
		if self.render_ctx.is_some() {
			panic!("Puppet already initialized for rendering.");
		}

		let render_ctx = RenderCtx::new(self);
		self.render_ctx = Some(render_ctx);
	}

	/// Call this on a puppet if params are going to be used. Panicks:
	/// - if rendering is not initialized.
	/// - on second call.
	pub fn init_params(&mut self) {
		if self.render_ctx.is_none() {
			panic!("Only a puppet initialized for rendering can be animated by params.");
		}
		if self.param_ctx.is_some() {
			panic!("Puppet already initialized for params.");
		}

		let param_ctx = ParamCtx::new(self);
		self.param_ctx = Some(param_ctx);
	}

	/// Call this on a puppet if physics are going to be simulated. Panicks:
	/// - if params is not initialized.
	/// - on second call.
	pub fn init_physics(&mut self) {
		if self.param_ctx.is_none() {
			panic!("Puppet physics depends on initialized puppet params.");
		}
		if self.physics_ctx.is_some() {
			panic!("Puppet already initialized for physics.");
		}

		let physics_ctx = PhysicsCtx::new(self);
		self.physics_ctx = Some(physics_ctx);
	}

	/// Apply a physics-only transform offset to a node.
	///
	/// The offset participates in the transform sampled by physics, but does not remain in the final rendered pose.
	pub fn set_physics_input_offset(
		&mut self,
		node: InoxNodeUuid,
		offset: TransformOffset,
	) -> Result<(), SetPhysicsInputOffsetError> {
		if self.nodes.get_node(node).is_none() {
			return Err(SetPhysicsInputOffsetError::NoNodeWithUuid(node.0));
		}

		if is_identity_offset(&offset) {
			self.physics_input_offsets.remove(&node);
		} else {
			self.physics_input_offsets.insert(node, offset);
		}

		Ok(())
	}

	/// Convenience wrapper around [`Puppet::set_physics_input_offset`] using a node name lookup.
	pub fn set_physics_input_offset_by_name(
		&mut self,
		node_name: &str,
		offset: TransformOffset,
	) -> Result<(), SetPhysicsInputOffsetError> {
		let Some(node) = self.nodes.find_node_by_name(node_name) else {
			return Err(SetPhysicsInputOffsetError::NoNodeNamed(node_name.to_owned()));
		};

		self.set_physics_input_offset(node, offset)
	}

	/// Resolve the first available node from a list of candidate names and apply a physics-only transform offset.
	pub fn set_physics_input_offset_by_name_candidates(
		&mut self,
		node_names: &[&str],
		offset: TransformOffset,
	) -> Result<InoxNodeUuid, SetPhysicsInputOffsetError> {
		let Some(node) = self.nodes.find_first_node_by_names(node_names) else {
			return Err(SetPhysicsInputOffsetError::NoNodesNamed(
				node_names.iter().map(|name| (*name).to_owned()).collect(),
			));
		};

		self.set_physics_input_offset(node, offset)?;
		Ok(node)
	}

	/// Apply the same physics-only transform offset to every node whose name matches any candidate.
	pub fn set_physics_input_offsets_by_names(
		&mut self,
		node_names: &[&str],
		offset: TransformOffset,
	) -> Result<Vec<InoxNodeUuid>, SetPhysicsInputOffsetError> {
		let nodes = self.nodes.find_nodes_by_names(node_names);
		if nodes.is_empty() {
			return Err(SetPhysicsInputOffsetError::NoNodesNamed(
				node_names.iter().map(|name| (*name).to_owned()).collect(),
			));
		}

		for node in &nodes {
			self.set_physics_input_offset(*node, offset.clone())?;
		}

		Ok(nodes)
	}

	pub fn clear_physics_input_offsets(&mut self) {
		self.physics_input_offsets.clear();
	}

	/// Apply parameter overrides after physics and rebuild render transforms for the current frame.
	///
	/// This is intended for runtime-side presentation adjustments that should win over
	/// physics output only for the final rendered pose.
	pub fn apply_post_physics_param_overrides(
		&mut self,
		overrides: &HashMap<String, Vec2>,
	) -> Result<(), SetParamError> {
		if overrides.is_empty() {
			return Ok(());
		}

		for (param_name, value) in overrides {
			self.param_ctx
				.as_mut()
				.expect("Post-physics param overrides depend on initialized params.")
				.set(param_name, *value)?;
		}

		self.render_ctx
			.as_mut()
			.expect("Post-physics param overrides depend on initialized rendering.")
			.reset(&self.nodes, &mut self.node_comps);
		self.transform_ctx
			.as_mut()
			.expect("Post-physics param overrides depend on initialized transforms.")
			.reset(&self.nodes, &mut self.node_comps);
		self.param_ctx
			.as_ref()
			.expect("Post-physics param overrides depend on initialized params.")
			.apply(&self.params, &self.nodes, &mut self.node_comps);
		self.transform_ctx
			.as_mut()
			.expect("Post-physics param overrides depend on initialized transforms.")
			.update(&self.nodes, &mut self.node_comps);
		self.render_ctx
			.as_mut()
			.expect("Post-physics param overrides depend on initialized rendering.")
			.update(&self.nodes, &mut self.node_comps);
		Ok(())
	}

	/// Apply visible transform offsets after physics for the final rendered pose.
	pub fn apply_post_physics_transform_offsets_by_names(
		&mut self,
		node_names: &[&str],
		offset: &TransformOffset,
	) -> Result<Vec<InoxNodeUuid>, SetPhysicsInputOffsetError> {
		let nodes = self.nodes.find_nodes_by_names(node_names);
		if nodes.is_empty() {
			return Err(SetPhysicsInputOffsetError::NoNodesNamed(
				node_names.iter().map(|name| (*name).to_owned()).collect(),
			));
		}

		for node in &nodes {
			let Some(transform) = self.node_comps.get_mut::<TransformStore>(*node) else {
				continue;
			};

			transform.relative.translation += offset.translation;
			transform.relative.rotation += offset.rotation;
			transform.relative.scale *= offset.scale;
			transform.relative.pixel_snap |= offset.pixel_snap;
		}

		self.transform_ctx
			.as_mut()
			.expect("Post-physics transform offsets depend on initialized transforms.")
			.update(&self.nodes, &mut self.node_comps);

		Ok(nodes)
	}

	/// Override drawable opacity for every node whose name matches any candidate.
	///
	/// This is intended for runtime motion effects such as Live2D `PartOpacity`.
	/// Call it after frame evaluation and before rendering so the value wins for the
	/// final pose without mutating the authored model.
	pub fn set_drawable_opacity_by_names(
		&mut self,
		node_names: &[&str],
		opacity: f32,
	) -> Result<Vec<InoxNodeUuid>, SetDrawableOpacityError> {
		let nodes = self.nodes.find_nodes_by_names(node_names);
		if nodes.is_empty() {
			return Err(SetDrawableOpacityError::NoNodesNamed(
				node_names.iter().map(|name| (*name).to_owned()).collect(),
			));
		}

		let opacity = if opacity.is_finite() {
			opacity.clamp(0.0, 1.0)
		} else {
			1.0
		};
		let mut drawable_nodes = Vec::new();
		for node in &nodes {
			let Some(drawable) = self.node_comps.get_mut::<Drawable>(*node) else {
				continue;
			};

			drawable.blending.opacity = opacity;
			drawable_nodes.push(*node);
		}

		if drawable_nodes.is_empty() {
			return Err(SetDrawableOpacityError::NoDrawableNodesNamed(
				node_names.iter().map(|name| (*name).to_owned()).collect(),
			));
		}

		Ok(drawable_nodes)
	}

	/// Alias for [`Puppet::set_drawable_opacity_by_names`] that matches the post-physics override API naming.
	pub fn apply_post_physics_drawable_opacity_by_names(
		&mut self,
		node_names: &[&str],
		opacity: f32,
	) -> Result<Vec<InoxNodeUuid>, SetDrawableOpacityError> {
		self.set_drawable_opacity_by_names(node_names, opacity)
	}

	fn apply_physics_input_offsets(&mut self) {
		for (node, offset) in &self.physics_input_offsets {
			let Some(transform) = self.node_comps.get_mut::<TransformStore>(*node) else {
				continue;
			};

			transform.relative.translation += offset.translation;
			transform.relative.rotation += offset.rotation;
			transform.relative.scale *= offset.scale;
			transform.relative.pixel_snap |= offset.pixel_snap;
		}
	}

	/// Prepare the puppet for a new frame. User may set params afterwards.
	pub fn begin_frame(&mut self) {
		if let Some(render_ctx) = self.render_ctx.as_mut() {
			render_ctx.reset(&self.nodes, &mut self.node_comps);
		}

		if let Some(transform_ctx) = self.transform_ctx.as_mut() {
			transform_ctx.reset(&self.nodes, &mut self.node_comps);
		}

		if let Some(param_ctx) = self.param_ctx.as_mut() {
			param_ctx.reset(&self.params);
		}
	}

	/// Freeze puppet for one frame. Rendering, if initialized, may follow.
	///
	/// Provide elapsed time for physics, if initialized, to run. Provide `0` for the first call.
	pub fn end_frame(&mut self, dt: f32) {
		if let Some(param_ctx) = self.param_ctx.as_mut() {
			param_ctx.apply(&self.params, &self.nodes, &mut self.node_comps);
		}

		if self.physics_ctx.is_some() {
			self.apply_physics_input_offsets();
		}

		if let Some(transform_ctx) = self.transform_ctx.as_mut() {
			transform_ctx.update(&self.nodes, &mut self.node_comps);
		}

		if let Some(physics_ctx) = self.physics_ctx.as_mut() {
			let values_to_apply = physics_ctx.step(&self.physics, &self.nodes, &mut self.node_comps, dt);

			// TODO: Think about separating DeformStack reset and RenderCtx reset?
			self.render_ctx
				.as_mut()
				.expect("If physics is initialized, so does params, so does rendering.")
				.reset(&self.nodes, &mut self.node_comps);

			// TODO: Fewer repeated calculations of a same transform?
			let transform_ctx = self
				.transform_ctx
				.as_mut()
				.expect("If physics is initialized, so does transforms.");
			transform_ctx.reset(&self.nodes, &mut self.node_comps);

			let param_ctx = self
				.param_ctx
				.as_mut()
				.expect("If physics is initialized, so does params.");
			for (param_name, value) in &values_to_apply {
				param_ctx
					.set(param_name, *value)
					.expect("Param name returned by .step() must exist.");
			}
			param_ctx.apply(&self.params, &self.nodes, &mut self.node_comps);

			transform_ctx.update(&self.nodes, &mut self.node_comps);
		}

		if let Some(render_ctx) = self.render_ctx.as_mut() {
			render_ctx.update(&self.nodes, &mut self.node_comps);
		}
	}
}

fn is_identity_offset(offset: &TransformOffset) -> bool {
	offset.translation == Vec3::ZERO && offset.rotation == Vec3::ZERO && offset.scale == Vec2::ONE && !offset.pixel_snap
}

/// Possible errors configuring physics-only transform input.
#[derive(Debug, thiserror::Error)]
pub enum SetPhysicsInputOffsetError {
	#[error("No node named {0}")]
	NoNodeNamed(String),
	#[error("No nodes named any of {0:?}")]
	NoNodesNamed(Vec<String>),
	#[error("No node with uuid {0}")]
	NoNodeWithUuid(u32),
}

/// Possible errors applying drawable opacity overrides.
#[derive(Debug, thiserror::Error)]
pub enum SetDrawableOpacityError {
	#[error("No nodes named any of {0:?}")]
	NoNodesNamed(Vec<String>),
	#[error("No drawable nodes named any of {0:?}")]
	NoDrawableNodesNamed(Vec<String>),
}

#[cfg(test)]
mod tests {
	use std::collections::HashMap;

	use glam::{vec2, Vec2};

	use super::*;
	use crate::math::interp::InterpolateMode;
	use crate::math::matrix::Matrix2d;
	use crate::node::components::{BlendMode, Blending, Drawable};
	use crate::node::{InoxNode, InoxNodeUuid};
	use crate::params::{AxisPoints, Binding, BindingValues, Param, ParamUuid};
	use crate::physics::PuppetPhysics;
	use crate::puppet::meta::PuppetMeta;

	fn node(uuid: u32, name: &str) -> InoxNode {
		InoxNode {
			uuid: InoxNodeUuid(uuid),
			name: name.to_owned(),
			enabled: true,
			zsort: 0.0,
			trans_offset: TransformOffset::default(),
			lock_to_root: false,
		}
	}

	fn meta() -> PuppetMeta {
		PuppetMeta {
			name: None,
			version: crate::INOCHI2D_SPEC_VERSION.to_owned(),
			rigger: None,
			artist: None,
			rights: None,
			copyright: None,
			license_url: None,
			contact: None,
			reference: None,
			thumbnail_id: None,
			preserve_pixels: false,
		}
	}

	fn mouth_param(target: InoxNodeUuid) -> Param {
		Param {
			uuid: ParamUuid(10),
			name: "ParamMouthOpenY".to_owned(),
			is_vec2: false,
			min: Vec2::ZERO,
			max: Vec2::ONE,
			defaults: Vec2::ZERO,
			axis_points: AxisPoints {
				x: vec![0.0, 1.0],
				y: vec![0.0],
			},
			bindings: vec![Binding {
				node: target,
				is_set: Matrix2d::default_filled(2, 1, false),
				interpolate_mode: InterpolateMode::Linear,
				values: BindingValues::TransformTX(Matrix2d::from_slice_vecs(&[vec![0.0, 10.0]], false).unwrap()),
			}],
		}
	}

	fn opacity_param(target: InoxNodeUuid) -> Param {
		Param {
			uuid: ParamUuid(11),
			name: "ParamOpacity".to_owned(),
			is_vec2: false,
			min: Vec2::ZERO,
			max: Vec2::ONE,
			defaults: Vec2::ZERO,
			axis_points: AxisPoints {
				x: vec![0.0, 1.0],
				y: vec![0.0],
			},
			bindings: vec![Binding {
				node: target,
				is_set: Matrix2d::default_filled(2, 1, false),
				interpolate_mode: InterpolateMode::Linear,
				values: BindingValues::Opacity(Matrix2d::from_slice_vecs(&[vec![0.0, 0.75]], false).unwrap()),
			}],
		}
	}

	fn drawable(opacity: f32) -> Drawable {
		Drawable {
			blending: Blending {
				mode: BlendMode::Normal,
				tint: Vec3::ONE,
				screen_tint: Vec3::ZERO,
				opacity,
			},
			masks: None,
		}
	}

	#[test]
	fn post_physics_param_override_rebuilds_render_pose_from_reset_state() {
		let root = InoxNodeUuid(1);
		let mouth = InoxNodeUuid(2);
		let mut params = HashMap::new();
		params.insert("ParamMouthOpenY".to_owned(), mouth_param(mouth));
		let mut puppet = Puppet::new(
			meta(),
			PuppetPhysics {
				pixels_per_meter: 100.0,
				gravity: 9.8,
			},
			node(root.0, "Root"),
			params,
		);
		puppet.nodes.add(root, mouth, node(mouth.0, "Mouth"));
		puppet.init_transforms();
		puppet.init_rendering();
		puppet.init_params();

		puppet.begin_frame();
		puppet
			.param_ctx
			.as_mut()
			.unwrap()
			.set("ParamMouthOpenY", vec2(0.2, 0.0))
			.unwrap();
		puppet.end_frame(0.0);
		assert_eq!(
			puppet
				.node_comps
				.get::<TransformStore>(mouth)
				.unwrap()
				.relative
				.translation
				.x,
			2.0
		);

		let mut overrides = HashMap::new();
		overrides.insert("ParamMouthOpenY".to_owned(), vec2(0.8, 0.0));
		puppet.apply_post_physics_param_overrides(&overrides).unwrap();

		assert_eq!(
			puppet
				.node_comps
				.get::<TransformStore>(mouth)
				.unwrap()
				.relative
				.translation
				.x,
			8.0
		);
	}

	#[test]
	fn opacity_param_applies_delta_from_base_and_resets_each_frame() {
		let root = InoxNodeUuid(1);
		let part = InoxNodeUuid(2);
		let mut params = HashMap::new();
		params.insert("ParamOpacity".to_owned(), opacity_param(part));
		let mut puppet = Puppet::new(
			meta(),
			PuppetPhysics {
				pixels_per_meter: 100.0,
				gravity: 9.8,
			},
			node(root.0, "Root"),
			params,
		);
		puppet.nodes.add(root, part, node(part.0, "Mouth"));
		puppet.node_comps.add(part, drawable(0.5));
		puppet.init_transforms();
		puppet.init_rendering();
		puppet.init_params();

		puppet.begin_frame();
		puppet.param_ctx.as_mut().unwrap().set("ParamOpacity", Vec2::X).unwrap();
		puppet.end_frame(0.0);
		assert_eq!(puppet.node_comps.get::<Drawable>(part).unwrap().blending.opacity, 1.0);

		puppet.begin_frame();
		puppet.end_frame(0.0);
		assert_eq!(puppet.node_comps.get::<Drawable>(part).unwrap().blending.opacity, 0.5);
	}

	#[test]
	fn set_drawable_opacity_by_names_clamps_and_does_not_change_base_opacity() {
		let root = InoxNodeUuid(1);
		let part = InoxNodeUuid(2);
		let mut puppet = Puppet::new(
			meta(),
			PuppetPhysics {
				pixels_per_meter: 100.0,
				gravity: 9.8,
			},
			node(root.0, "Root"),
			HashMap::new(),
		);
		puppet.nodes.add(root, part, node(part.0, "Mouth"));
		puppet.node_comps.add(part, drawable(0.5));
		puppet.init_transforms();
		puppet.init_rendering();

		assert!(puppet.set_drawable_opacity_by_names(&["Mouth"], 2.0).unwrap() == vec![part]);
		assert_eq!(puppet.node_comps.get::<Drawable>(part).unwrap().blending.opacity, 1.0);

		puppet.begin_frame();
		puppet.end_frame(0.0);
		assert_eq!(puppet.node_comps.get::<Drawable>(part).unwrap().blending.opacity, 0.5);
	}

	#[test]
	fn frame_pose_skeleton_keeps_phase_state_separate() {
		use crate::params::ParamUuid;
		use crate::puppet::frame_api::{
			FrameContext, FrameNodeHandle, FrameOverrideMode, FrameParamHandle, FrameParamOverride, FramePose,
		};

		let param = FrameParamHandle {
			uuid: ParamUuid(10),
			name: "ParamMouthOpenY".to_owned(),
		};
		let node = FrameNodeHandle { uuid: InoxNodeUuid(2) };
		let mut pose = FramePose::default();

		pose.base_params.push((param.clone(), vec2(0.2, 0.0)));
		pose.physics_outputs.push((param.clone(), vec2(0.4, 0.0)));
		pose.post_physics_param_overrides.push(FrameParamOverride {
			param,
			value: vec2(0.8, 0.0),
			mode: FrameOverrideMode::Replace,
		});
		pose.post_physics_transform_offsets
			.push((node, TransformOffset::default()));

		assert_eq!(pose.base_params.len(), 1);
		assert_eq!(pose.physics_outputs.len(), 1);
		assert_eq!(pose.post_physics_param_overrides.len(), 1);
		assert_eq!(pose.post_physics_transform_offsets.len(), 1);

		let context = FrameContext {
			pose,
			dt: 1.0 / 60.0,
			..FrameContext::default()
		};
		assert_eq!(context.pose.base_params.len(), 1);
		assert_eq!(context.dt, 1.0 / 60.0);
	}
}
