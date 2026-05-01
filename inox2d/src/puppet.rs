pub mod meta;
mod transforms;
mod tree;
mod world;

use std::collections::{HashMap, HashSet};

use glam::{Mat4, Vec2, Vec3};

use crate::math::transform::TransformOffset;
use crate::node::components::{Drawable, TransformStore, ZSort};
use crate::node::{InoxNode, InoxNodeUuid};
use crate::params::{Param, ParamCtx, ParamUuid, SetParamError};
use crate::physics::{PhysicsCtx, PuppetPhysics};
use crate::render::{CompositeRenderCtx, RenderCtx, TexturedMeshRenderCtx};

use meta::PuppetMeta;
use transforms::TransformCtx;
pub use tree::InoxNodeTree;
pub use world::World;

/// Opaque resolved node target for per-frame runtime effects.
///
/// Host integrations can resolve node names once when a model is loaded, then
/// reuse this handle for physics input, post-physics visible offsets, and
/// drawable opacity effects.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ResolvedNodeHandle {
	uuid: InoxNodeUuid,
}

impl ResolvedNodeHandle {
	pub fn raw_uuid(self) -> u32 {
		self.uuid.0
	}
}

/// Opaque resolved parameter target for per-frame runtime values.
///
/// Host integrations can resolve authored parameter names once when a model is
/// loaded, then reuse this handle for motion application without repeatedly
/// routing through public string identifiers.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ResolvedParamHandle {
	uuid: ParamUuid,
	name: String,
}

impl ResolvedParamHandle {
	pub fn raw_uuid(&self) -> u32 {
		self.uuid.0
	}

	pub fn name(&self) -> &str {
		&self.name
	}
}

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
	frame_context: frame_api::FrameContext,
	post_physics_offset_nodes: HashSet<InoxNodeUuid>,
}

pub mod frame_api {
	use glam::{Mat4, Vec2};

	use crate::math::transform::TransformOffset;
	use crate::node::InoxNodeUuid;
	use crate::params::ParamUuid;

	#[derive(Debug, Clone, Copy, PartialEq, Eq)]
	pub enum FrameOverrideMode {
		Replace,
		Add,
		Clamp,
	}

	#[derive(Debug, Clone, Copy, PartialEq, Eq)]
	pub enum FrameSkipPhase {
		InitRender,
		UpdateRender,
		CommitPose,
	}

	#[derive(Debug, Clone, Copy, PartialEq, Eq)]
	pub enum FrameSkipReason {
		Disabled,
		NonRenderable,
		MissingMesh,
		MissingDeformStack,
		CompositeChildExcludedFromRootDrawList,
	}

	#[derive(Debug, Clone, PartialEq, Eq)]
	pub struct FrameParamHandle {
		pub uuid: ParamUuid,
		pub name: String,
	}

	#[derive(Clone, Copy, PartialEq, Eq)]
	pub struct FrameNodeHandle {
		pub uuid: InoxNodeUuid,
	}

	impl std::fmt::Debug for FrameNodeHandle {
		fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
			f.debug_tuple("FrameNodeHandle").field(&self.uuid.0).finish()
		}
	}

	#[derive(Debug, Clone)]
	pub struct FrameParamOverride {
		pub param: FrameParamHandle,
		pub value: Vec2,
		pub mode: FrameOverrideMode,
	}

	#[derive(Debug, Default, Clone)]
	pub struct FramePose {
		pub base_params: Vec<(FrameParamHandle, Vec2)>,
		pub physics_input_offsets: Vec<(FrameNodeHandle, TransformOffset)>,
		pub physics_outputs: Vec<(FrameParamHandle, Vec2)>,
		pub post_physics_param_overrides: Vec<FrameParamOverride>,
		pub post_physics_transform_offsets: Vec<(FrameNodeHandle, TransformOffset)>,
	}

	#[derive(Debug, Clone, PartialEq, Eq)]
	pub struct FrameDiagnostic {
		pub phase: FrameSkipPhase,
		pub node: Option<FrameNodeHandle>,
		pub message: String,
	}

	#[derive(Debug, Default, Clone)]
	pub struct FrameContext {
		pub frame_id: u64,
		pub dt: f32,
		pub physics_ran: bool,
		pub pose: FramePose,
		pub diagnostics: Vec<FrameDiagnostic>,
	}

	#[derive(Debug, Clone)]
	pub struct FrameNodeSnapshot {
		pub node: FrameNodeHandle,
		pub name: String,
		pub enabled: bool,
		pub parent: Option<FrameNodeHandle>,
		pub relative_transform: TransformOffset,
		pub absolute_transform: Mat4,
		pub zsort: f32,
		pub post_physics_offset_applied: bool,
	}

	#[derive(Debug, Clone, PartialEq, Eq)]
	pub struct FrameDrawableSnapshot {
		pub node: FrameNodeHandle,
		pub vertex_range: std::ops::Range<usize>,
		pub index_range: std::ops::Range<usize>,
		pub deform_range: std::ops::Range<usize>,
	}

	#[derive(Debug, Clone, PartialEq, Eq)]
	pub struct FrameSkippedNode {
		pub node: FrameNodeHandle,
		pub name: String,
		pub phase: FrameSkipPhase,
		pub reason: FrameSkipReason,
	}

	#[derive(Debug, Default, Clone)]
	pub struct FrameSnapshot {
		pub nodes: Vec<FrameNodeSnapshot>,
		pub root_drawables_zsorted: Vec<FrameNodeHandle>,
		pub composite_children_zsorted: Vec<(FrameNodeHandle, Vec<FrameNodeHandle>)>,
		pub drawables: Vec<FrameDrawableSnapshot>,
		pub skipped_nodes: Vec<FrameSkippedNode>,
	}
}

impl From<ResolvedNodeHandle> for frame_api::FrameNodeHandle {
	fn from(handle: ResolvedNodeHandle) -> Self {
		Self { uuid: handle.uuid }
	}
}

impl From<&ResolvedParamHandle> for frame_api::FrameParamHandle {
	fn from(handle: &ResolvedParamHandle) -> Self {
		Self {
			uuid: handle.uuid,
			name: handle.name.clone(),
		}
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
			frame_context: frame_api::FrameContext::default(),
			post_physics_offset_nodes: HashSet::new(),
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

	pub fn resolve_node_by_name(&self, node_name: &str) -> Result<ResolvedNodeHandle, SetPhysicsInputOffsetError> {
		let Some(node) = self.nodes.find_node_by_name(node_name) else {
			return Err(SetPhysicsInputOffsetError::NoNodeNamed(node_name.to_owned()));
		};

		Ok(ResolvedNodeHandle { uuid: node })
	}

	pub fn resolve_nodes_by_names(
		&self,
		node_names: &[&str],
	) -> Result<Vec<ResolvedNodeHandle>, SetPhysicsInputOffsetError> {
		let nodes = self.nodes.find_nodes_by_names(node_names);
		if nodes.is_empty() {
			return Err(SetPhysicsInputOffsetError::NoNodesNamed(
				node_names.iter().map(|name| (*name).to_owned()).collect(),
			));
		}

		Ok(nodes.into_iter().map(|uuid| ResolvedNodeHandle { uuid }).collect())
	}

	pub fn resolve_param_by_name(&self, param_name: &str) -> Result<ResolvedParamHandle, SetParamError> {
		let Some(param) = self.params.get(param_name) else {
			return Err(SetParamError::NoParameterNamed(param_name.to_owned()));
		};

		Ok(ResolvedParamHandle {
			uuid: param.uuid,
			name: param_name.to_owned(),
		})
	}

	pub fn set_parameter_by_handle(&mut self, param: &ResolvedParamHandle, value: Vec2) -> Result<(), SetParamError> {
		self.validate_param_handle(param)?;
		self.param_ctx
			.as_mut()
			.expect("Resolved parameter updates depend on initialized params.")
			.set(param.name(), value)?;
		self.frame_context
			.pose
			.base_params
			.push((frame_api::FrameParamHandle::from(param), value));
		Ok(())
	}

	pub fn set_physics_input_offset_by_handle(
		&mut self,
		node: ResolvedNodeHandle,
		offset: TransformOffset,
	) -> Result<(), SetPhysicsInputOffsetError> {
		self.set_physics_input_offset(node.uuid, offset.clone())?;
		self.frame_context
			.pose
			.physics_input_offsets
			.push((frame_api::FrameNodeHandle::from(node), offset));
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

		self.set_physics_input_offset(node, offset.clone())?;
		self.frame_context
			.pose
			.physics_input_offsets
			.push((frame_api::FrameNodeHandle { uuid: node }, offset));
		Ok(())
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

		self.set_physics_input_offset(node, offset.clone())?;
		self.frame_context
			.pose
			.physics_input_offsets
			.push((frame_api::FrameNodeHandle { uuid: node }, offset));
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
			self.frame_context
				.pose
				.physics_input_offsets
				.push((frame_api::FrameNodeHandle { uuid: *node }, offset.clone()));
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
			let handle = self.resolve_param_by_name(param_name)?;
			self.frame_context
				.pose
				.post_physics_param_overrides
				.push(frame_api::FrameParamOverride {
					param: frame_api::FrameParamHandle::from(&handle),
					value: *value,
					mode: frame_api::FrameOverrideMode::Replace,
				});
		}

		self.rebuild_render_pose_from_params();
		Ok(())
	}

	pub fn apply_post_physics_param_overrides_by_handles(
		&mut self,
		overrides: &[(ResolvedParamHandle, Vec2)],
	) -> Result<(), SetParamError> {
		if overrides.is_empty() {
			return Ok(());
		}

		for (param, value) in overrides {
			self.validate_param_handle(param)?;
			self.param_ctx
				.as_mut()
				.expect("Post-physics param overrides depend on initialized params.")
				.set(param.name(), *value)?;
			self.frame_context
				.pose
				.post_physics_param_overrides
				.push(frame_api::FrameParamOverride {
					param: frame_api::FrameParamHandle::from(param),
					value: *value,
					mode: frame_api::FrameOverrideMode::Replace,
				});
		}

		self.rebuild_render_pose_from_params();
		Ok(())
	}

	fn validate_param_handle(&self, param: &ResolvedParamHandle) -> Result<(), SetParamError> {
		let Some(current) = self.params.get(param.name()) else {
			return Err(SetParamError::NoParameterWithUuid(param.raw_uuid()));
		};
		if current.uuid != param.uuid {
			return Err(SetParamError::NoParameterWithUuid(param.raw_uuid()));
		}

		Ok(())
	}

	fn rebuild_render_pose_from_params(&mut self) {
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
			self.post_physics_offset_nodes.insert(*node);
			self.frame_context
				.pose
				.post_physics_transform_offsets
				.push((frame_api::FrameNodeHandle { uuid: *node }, (*offset).clone()));
		}

		self.transform_ctx
			.as_mut()
			.expect("Post-physics transform offsets depend on initialized transforms.")
			.update(&self.nodes, &mut self.node_comps);

		Ok(nodes)
	}

	pub fn apply_post_physics_transform_offsets_by_handles(
		&mut self,
		nodes: &[ResolvedNodeHandle],
		offset: &TransformOffset,
	) -> Result<Vec<ResolvedNodeHandle>, SetPhysicsInputOffsetError> {
		let mut applied = Vec::new();
		for node in nodes {
			if self.nodes.get_node(node.uuid).is_none() {
				return Err(SetPhysicsInputOffsetError::NoNodeWithUuid(node.raw_uuid()));
			}

			let Some(transform) = self.node_comps.get_mut::<TransformStore>(node.uuid) else {
				continue;
			};

			transform.relative.translation += offset.translation;
			transform.relative.rotation += offset.rotation;
			transform.relative.scale *= offset.scale;
			transform.relative.pixel_snap |= offset.pixel_snap;
			self.post_physics_offset_nodes.insert(node.uuid);
			self.frame_context
				.pose
				.post_physics_transform_offsets
				.push((frame_api::FrameNodeHandle::from(*node), (*offset).clone()));
			applied.push(*node);
		}

		self.transform_ctx
			.as_mut()
			.expect("Post-physics transform offsets depend on initialized transforms.")
			.update(&self.nodes, &mut self.node_comps);

		Ok(applied)
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

	pub fn set_drawable_opacity_by_handles(
		&mut self,
		nodes: &[ResolvedNodeHandle],
		opacity: f32,
	) -> Result<Vec<ResolvedNodeHandle>, SetDrawableOpacityError> {
		let opacity = if opacity.is_finite() {
			opacity.clamp(0.0, 1.0)
		} else {
			1.0
		};
		let mut drawable_nodes = Vec::new();
		for node in nodes {
			if self.nodes.get_node(node.uuid).is_none() {
				return Err(SetDrawableOpacityError::NoNodeWithUuid(node.raw_uuid()));
			}

			let Some(drawable) = self.node_comps.get_mut::<Drawable>(node.uuid) else {
				continue;
			};

			drawable.blending.opacity = opacity;
			drawable_nodes.push(*node);
		}

		if drawable_nodes.is_empty() {
			return Err(SetDrawableOpacityError::NoDrawableNodes);
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
		let next_frame_id = self.frame_context.frame_id.wrapping_add(1);
		self.frame_context = frame_api::FrameContext {
			frame_id: next_frame_id,
			..frame_api::FrameContext::default()
		};
		self.post_physics_offset_nodes.clear();

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
		self.frame_context.dt = dt;
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
			self.frame_context.physics_ran = true;
			let values_to_apply = physics_ctx.step(&self.physics, &mut self.node_comps, dt);

			// TODO: Think about separating DeformStack reset and RenderCtx reset?
			self.render_ctx
				.as_mut()
				.expect("If physics is initialized, so does params, so does rendering.")
				.reset(&self.nodes, &mut self.node_comps);

			for (param_name, value) in &values_to_apply {
				if let Some(param) = self.params.get(param_name) {
					self.frame_context.pose.physics_outputs.push((
						frame_api::FrameParamHandle {
							uuid: param.uuid,
							name: param_name.to_owned(),
						},
						*value,
					));
				}
			}

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

	pub fn frame_context(&self) -> &frame_api::FrameContext {
		&self.frame_context
	}

	pub fn snapshot_frame(&self) -> frame_api::FrameSnapshot {
		let nodes = self
			.nodes
			.pre_order_iter()
			.map(|node| {
				let parent = if node.uuid == self.nodes.root_node_id {
					None
				} else {
					Some(frame_api::FrameNodeHandle {
						uuid: self.nodes.get_parent(node.uuid).uuid,
					})
				};
				let transform = self.node_comps.get::<TransformStore>(node.uuid);
				let zsort = self
					.node_comps
					.get::<ZSort>(node.uuid)
					.map(|zsort| zsort.0)
					.unwrap_or(node.zsort);

				frame_api::FrameNodeSnapshot {
					node: frame_api::FrameNodeHandle { uuid: node.uuid },
					name: node.name.clone(),
					enabled: node.enabled,
					parent,
					relative_transform: transform.map(|store| store.relative.clone()).unwrap_or_default(),
					absolute_transform: transform.map(|store| store.absolute).unwrap_or(Mat4::IDENTITY),
					zsort,
					post_physics_offset_applied: self.post_physics_offset_nodes.contains(&node.uuid),
				}
			})
			.collect();

		let root_drawables_zsorted = self
			.render_ctx
			.as_ref()
			.map(|render_ctx| {
				render_ctx
					.root_drawables_zsorted()
					.iter()
					.map(|uuid| frame_api::FrameNodeHandle { uuid: *uuid })
					.collect()
			})
			.unwrap_or_default();

		let mut composite_children_zsorted = Vec::new();
		let mut drawables = Vec::new();
		let mut skipped_nodes = Vec::new();

		for node in self.nodes.pre_order_iter() {
			if !node.enabled {
				skipped_nodes.push(frame_api::FrameSkippedNode {
					node: frame_api::FrameNodeHandle { uuid: node.uuid },
					name: node.name.clone(),
					phase: frame_api::FrameSkipPhase::CommitPose,
					reason: frame_api::FrameSkipReason::Disabled,
				});
				continue;
			}

			if let Some(composite) = self.node_comps.get::<CompositeRenderCtx>(node.uuid) {
				composite_children_zsorted.push((
					frame_api::FrameNodeHandle { uuid: node.uuid },
					composite
						.zsorted_children_list
						.iter()
						.map(|uuid| frame_api::FrameNodeHandle { uuid: *uuid })
						.collect(),
				));
			}

			if let Some(render_ctx) = self.node_comps.get::<TexturedMeshRenderCtx>(node.uuid) {
				let vert_offset = render_ctx.vert_offset as usize;
				let index_offset = render_ctx.index_offset as usize;
				drawables.push(frame_api::FrameDrawableSnapshot {
					node: frame_api::FrameNodeHandle { uuid: node.uuid },
					vertex_range: vert_offset..(vert_offset + render_ctx.vert_len),
					index_range: index_offset..(index_offset + render_ctx.index_len),
					deform_range: vert_offset..(vert_offset + render_ctx.vert_len),
				});
			}
		}

		frame_api::FrameSnapshot {
			nodes,
			root_drawables_zsorted,
			composite_children_zsorted,
			drawables,
			skipped_nodes,
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
	#[error("No node with uuid {0}")]
	NoNodeWithUuid(u32),
	#[error("No drawable nodes in resolved handle list")]
	NoDrawableNodes,
}

#[cfg(test)]
mod tests {
	use std::collections::HashMap;

	use glam::{vec2, Vec2};

	use super::*;
	use crate::math::interp::InterpolateMode;
	use crate::math::matrix::Matrix2d;
	use crate::node::components::{BlendMode, Blending, Drawable, Mesh, TexturedMesh};
	use crate::node::{InoxNode, InoxNodeUuid};
	use crate::params::{AxisPoints, Binding, BindingValues, Param, ParamUuid};
	use crate::physics::PuppetPhysics;
	use crate::puppet::meta::PuppetMeta;
	use crate::texture::TextureId;

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

	fn add_mesh_drawable(puppet: &mut Puppet, id: InoxNodeUuid, opacity: f32) {
		puppet.node_comps.add(id, drawable(opacity));
		puppet.node_comps.add(
			id,
			TexturedMesh {
				tex_albedo: TextureId(0),
				tex_emissive: TextureId(0),
				tex_bumpmap: TextureId(0),
			},
		);
		puppet.node_comps.add(
			id,
			Mesh {
				vertices: vec![Vec2::ZERO, Vec2::X, Vec2::Y],
				uvs: vec![Vec2::ZERO, Vec2::X, Vec2::Y],
				indices: vec![0, 1, 2],
				origin: Vec2::ZERO,
			},
		);
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
	fn resolved_param_handles_set_frame_values_without_public_name_lookup() {
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

		let handle = puppet.resolve_param_by_name("ParamMouthOpenY").unwrap();
		assert_eq!(handle.raw_uuid(), 10);

		puppet.begin_frame();
		puppet.set_parameter_by_handle(&handle, vec2(0.6, 0.0)).unwrap();
		puppet.end_frame(0.0);

		assert_eq!(
			puppet
				.node_comps
				.get::<TransformStore>(mouth)
				.unwrap()
				.relative
				.translation
				.x,
			6.0
		);
	}

	#[test]
	fn resolved_param_handles_apply_post_physics_overrides() {
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

		let handle = puppet.resolve_param_by_name("ParamMouthOpenY").unwrap();
		puppet.begin_frame();
		puppet.end_frame(0.0);
		puppet
			.apply_post_physics_param_overrides_by_handles(&[(handle, vec2(0.7, 0.0))])
			.unwrap();

		assert_eq!(
			puppet
				.node_comps
				.get::<TransformStore>(mouth)
				.unwrap()
				.relative
				.translation
				.x,
			7.0
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
	fn resolved_node_handles_apply_drawable_opacity_without_name_lookup() {
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
		puppet.nodes.add(root, part, node(part.0, "Arm:: Left"));
		puppet.node_comps.add(part, drawable(0.5));
		puppet.init_transforms();
		puppet.init_rendering();

		let handle = puppet.resolve_node_by_name("Arm:: Left").unwrap();
		assert_eq!(handle.raw_uuid(), part.0);

		let applied = puppet.set_drawable_opacity_by_handles(&[handle], 0.25).unwrap();
		assert_eq!(applied, vec![handle]);
		assert_eq!(puppet.node_comps.get::<Drawable>(part).unwrap().blending.opacity, 0.25);

		puppet.begin_frame();
		puppet.end_frame(0.0);
		assert_eq!(puppet.node_comps.get::<Drawable>(part).unwrap().blending.opacity, 0.5);
	}

	#[test]
	fn resolved_node_handles_apply_visible_offsets_without_name_lookup() {
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
		puppet.nodes.add(root, part, node(part.0, "Hand:: Left"));
		puppet.init_transforms();

		let handle = puppet.resolve_node_by_name("Hand:: Left").unwrap();
		let mut offset = TransformOffset::default();
		offset.translation.x = 4.0;
		let applied = puppet
			.apply_post_physics_transform_offsets_by_handles(&[handle], &offset)
			.unwrap();

		assert_eq!(applied, vec![handle]);
		assert_eq!(
			puppet
				.node_comps
				.get::<TransformStore>(part)
				.unwrap()
				.relative
				.translation
				.x,
			4.0
		);
	}

	#[test]
	fn frame_context_records_runtime_phase_inputs() {
		let root = InoxNodeUuid(1);
		let mouth = InoxNodeUuid(2);
		let hand = InoxNodeUuid(3);
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
		puppet.nodes.add(root, hand, node(hand.0, "Hand:: Left"));
		puppet.init_transforms();
		puppet.init_rendering();
		puppet.init_params();

		let param = puppet.resolve_param_by_name("ParamMouthOpenY").unwrap();
		let hand_handle = puppet.resolve_node_by_name("Hand:: Left").unwrap();
		let mut offset = TransformOffset::default();
		offset.translation.x = 3.0;

		puppet.begin_frame();
		puppet.set_parameter_by_handle(&param, vec2(0.2, 0.0)).unwrap();
		puppet
			.set_physics_input_offset_by_handle(hand_handle, offset.clone())
			.unwrap();
		puppet.end_frame(0.016);
		puppet
			.apply_post_physics_param_overrides_by_handles(&[(param, vec2(0.4, 0.0))])
			.unwrap();
		puppet
			.apply_post_physics_transform_offsets_by_handles(&[hand_handle], &offset)
			.unwrap();

		let context = puppet.frame_context();
		assert_eq!(context.frame_id, 1);
		assert_eq!(context.dt, 0.016);
		assert_eq!(context.pose.base_params.len(), 1);
		assert_eq!(context.pose.physics_input_offsets.len(), 1);
		assert_eq!(context.pose.post_physics_param_overrides.len(), 1);
		assert_eq!(context.pose.post_physics_transform_offsets.len(), 1);
	}

	#[test]
	fn snapshot_frame_exposes_final_transforms_and_draw_order() {
		let root = InoxNodeUuid(1);
		let enabled = InoxNodeUuid(2);
		let disabled = InoxNodeUuid(3);
		let mut puppet = Puppet::new(
			meta(),
			PuppetPhysics {
				pixels_per_meter: 100.0,
				gravity: 9.8,
			},
			node(root.0, "Root"),
			HashMap::new(),
		);
		puppet.nodes.add(root, enabled, node(enabled.0, "EnabledPart"));
		let mut disabled_node = node(disabled.0, "DisabledPart");
		disabled_node.enabled = false;
		puppet.nodes.add(root, disabled, disabled_node);
		add_mesh_drawable(&mut puppet, enabled, 1.0);
		add_mesh_drawable(&mut puppet, disabled, 1.0);
		puppet.init_transforms();
		puppet.init_rendering();

		let handle = puppet.resolve_node_by_name("EnabledPart").unwrap();
		let mut offset = TransformOffset::default();
		offset.translation.x = 5.0;

		puppet.begin_frame();
		puppet.end_frame(0.0);
		puppet
			.apply_post_physics_transform_offsets_by_handles(&[handle], &offset)
			.unwrap();

		let snapshot = puppet.snapshot_frame();
		assert_eq!(
			snapshot.root_drawables_zsorted,
			vec![frame_api::FrameNodeHandle { uuid: enabled }]
		);
		assert_eq!(snapshot.drawables.len(), 1);
		assert_eq!(snapshot.skipped_nodes.len(), 1);
		let enabled_snapshot = snapshot.nodes.iter().find(|node| node.node.uuid == enabled).unwrap();
		assert!(enabled_snapshot.post_physics_offset_applied);
		assert_eq!(enabled_snapshot.relative_transform.translation.x, 5.0);
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
