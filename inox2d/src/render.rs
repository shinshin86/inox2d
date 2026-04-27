mod deform_stack;
mod vertex_buffers;

use std::collections::HashSet;
use std::mem::swap;

use crate::node::{
	components::{DeformStack, Drawable, DrawableBaseOpacity, Mask, Masks, ZSort},
	drawables::{CompositeComponents, DrawableKind, TexturedMeshComponents},
	InoxNodeUuid,
};
use crate::params::BindingValues;
use crate::puppet::{InoxNodeTree, Puppet, World};

pub use vertex_buffers::VertexBuffers;

/// Additional info per node for rendering a TexturedMesh:
/// - offset and length of array for mesh point coordinates
/// - offset and length of array for indices of mesh points defining the mesh
///
/// inside `puppet.render_ctx_vertex_buffers`.
pub struct TexturedMeshRenderCtx {
	pub index_offset: u32,
	pub vert_offset: u32,
	pub index_len: usize,
	pub vert_len: usize,
}

/// Additional info per node for rendering a Composite.
pub struct CompositeRenderCtx {
	pub zsorted_children_list: Vec<InoxNodeUuid>,
}

/// Additional struct attached to a puppet for rendering.
pub struct RenderCtx {
	/// General compact data buffers for interfacing with the GPU.
	pub vertex_buffers: VertexBuffers,
	/// All nodes that need respective draw method calls:
	/// - including standalone parts and composite parents,
	/// - excluding (TODO: plain mesh masks) and composite children.
	root_drawables_zsorted: Vec<InoxNodeUuid>,
}

impl RenderCtx {
	/// MODIFIES puppet. In addition to initializing self, installs render contexts in the World of components
	pub(super) fn new(puppet: &mut Puppet) -> Self {
		let nodes = &puppet.nodes;
		let comps = &mut puppet.node_comps;

		let mut nodes_to_deform = HashSet::new();
		for param in &puppet.params {
			param.1.bindings.iter().for_each(|b| {
				if matches!(b.values, BindingValues::Deform(_)) {
					nodes_to_deform.insert(b.node);
				}
			});
		}
		// TODO: Further fill the set when Meshgroup is implemented.

		let mut vertex_buffers = VertexBuffers::default();

		let mut root_drawables_count: usize = 0;
		for node in nodes.iter() {
			if !node.enabled {
				continue;
			}

			if let Some(base_opacity) = comps
				.get::<Drawable>(node.uuid)
				.map(|drawable| drawable.blending.opacity)
			{
				comps.add(node.uuid, DrawableBaseOpacity(base_opacity));
			}

			let drawable_kind = DrawableKind::new(node.uuid, comps, true);
			if let Some(drawable_kind) = drawable_kind {
				root_drawables_count += 1;

				match drawable_kind {
					DrawableKind::TexturedMesh(components) => {
						let (index_offset, vert_offset) = vertex_buffers.push(components.mesh);
						let (index_len, vert_len) = (components.mesh.indices.len(), components.mesh.vertices.len());

						comps.add(
							node.uuid,
							TexturedMeshRenderCtx {
								index_offset,
								vert_offset,
								index_len,
								vert_len,
							},
						);

						// TexturedMesh not deformed by any source does not need a DeformStack
						if nodes_to_deform.contains(&node.uuid) {
							comps.add(node.uuid, DeformStack::new(vert_len));
						}
					}
					DrawableKind::Composite { .. } => {
						// exclude non-drawable children
						let children_list: Vec<InoxNodeUuid> = nodes
							.get_children(node.uuid)
							.filter_map(|n| {
								if n.enabled && DrawableKind::new(n.uuid, comps, false).is_some() {
									Some(n.uuid)
								} else {
									None
								}
							})
							.collect();

						// composite children are excluded from root_drawables_zsorted
						root_drawables_count -= children_list.len();

						comps.add(
							node.uuid,
							CompositeRenderCtx {
								// sort later, before render
								zsorted_children_list: children_list,
							},
						);
					}
				};
			}
		}

		let mut root_drawables_zsorted = Vec::new();
		// similarly, populate later, before render
		root_drawables_zsorted.resize(root_drawables_count, InoxNodeUuid(0));

		Self {
			vertex_buffers,
			root_drawables_zsorted,
		}
	}

	/// Reset all `DeformStack`.
	pub(crate) fn reset(&mut self, nodes: &InoxNodeTree, comps: &mut World) {
		for node in nodes.iter() {
			if let Some(base_opacity) = comps.get::<DrawableBaseOpacity>(node.uuid).map(|base| base.0) {
				comps.get_mut::<Drawable>(node.uuid).unwrap().blending.opacity = base_opacity;
			}

			if let Some(deform_stack) = comps.get_mut::<DeformStack>(node.uuid) {
				deform_stack.reset();
			}
		}
	}

	/// Update zsort-ordered info and deform buffer content inside self, according to updated puppet.
	pub(crate) fn update(&mut self, nodes: &InoxNodeTree, comps: &mut World) {
		let mut root_drawable_uuid_zsort_vec = Vec::<(InoxNodeUuid, f32)>::new();

		// root is definitely not a drawable.
		for node in nodes.iter().skip(1) {
			if !node.enabled {
				continue;
			}

			if let Some(drawable_kind) = DrawableKind::new(node.uuid, comps, false) {
				let parent = nodes.get_parent(node.uuid);
				let node_zsort = comps.get::<ZSort>(node.uuid).unwrap().0;

				if !matches!(
					DrawableKind::new(parent.uuid, comps, false),
					Some(DrawableKind::Composite(_))
				) {
					// exclude composite children
					root_drawable_uuid_zsort_vec.push((node.uuid, node_zsort));
				}

				match drawable_kind {
					// for Composite, update zsorted children list
					DrawableKind::Composite { .. } => {
						// `swap()` usage is a trick that both:
						// - returns mut borrowed comps early
						// - does not involve any heap allocations
						let mut zsorted_children_list = Vec::new();
						swap(
							&mut zsorted_children_list,
							&mut comps
								.get_mut::<CompositeRenderCtx>(node.uuid)
								.unwrap()
								.zsorted_children_list,
						);

						zsorted_children_list.sort_by(|a, b| {
							let zsort_a = comps.get::<ZSort>(*a).unwrap();
							let zsort_b = comps.get::<ZSort>(*b).unwrap();
							zsort_a.total_cmp(zsort_b).reverse()
						});

						swap(
							&mut zsorted_children_list,
							&mut comps
								.get_mut::<CompositeRenderCtx>(node.uuid)
								.unwrap()
								.zsorted_children_list,
						);
					}
					// for TexturedMesh, obtain and write deforms into vertex_buffer
					DrawableKind::TexturedMesh(..) => {
						// A TexturedMesh not having an associated DeformStack means it will not be deformed at all, skip.
						if let Some(deform_stack) = comps.get::<DeformStack>(node.uuid) {
							let render_ctx = comps.get::<TexturedMeshRenderCtx>(node.uuid).unwrap();
							let vert_offset = render_ctx.vert_offset as usize;
							let vert_len = render_ctx.vert_len;
							deform_stack.combine(
								nodes,
								comps,
								&mut self.vertex_buffers.deforms[vert_offset..(vert_offset + vert_len)],
							);
						}
					}
				}
			}
		}

		root_drawable_uuid_zsort_vec.sort_by(|a, b| a.1.total_cmp(&b.1).reverse());
		self.root_drawables_zsorted
			.iter_mut()
			.zip(root_drawable_uuid_zsort_vec.iter())
			.for_each(|(old, new)| *old = new.0);
	}
}

#[cfg(test)]
mod tests {
	use std::collections::HashMap;

	use glam::{Vec2, Vec3};

	use super::*;
	use crate::math::transform::TransformOffset;
	use crate::node::{
		components::{BlendMode, Blending, Drawable, Mesh, TexturedMesh},
		InoxNode,
	};
	use crate::physics::PuppetPhysics;
	use crate::puppet::meta::PuppetMeta;
	use crate::texture::TextureId;

	fn node(uuid: u32, name: &str, enabled: bool) -> InoxNode {
		InoxNode {
			uuid: InoxNodeUuid(uuid),
			name: name.to_owned(),
			enabled,
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

	fn add_mesh_drawable(puppet: &mut Puppet, id: InoxNodeUuid) {
		puppet.node_comps.add(
			id,
			Drawable {
				blending: Blending {
					mode: BlendMode::Normal,
					tint: Vec3::ONE,
					screen_tint: Vec3::ZERO,
					opacity: 1.0,
				},
				masks: None,
			},
		);
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
	fn disabled_drawable_nodes_do_not_enter_render_buffers() {
		let root = InoxNodeUuid(1);
		let enabled = InoxNodeUuid(2);
		let disabled = InoxNodeUuid(3);
		let mut puppet = Puppet::new(
			meta(),
			PuppetPhysics {
				pixels_per_meter: 100.0,
				gravity: 9.8,
			},
			node(root.0, "Root", true),
			HashMap::new(),
		);
		puppet.nodes.add(root, enabled, node(enabled.0, "Mouth", true));
		puppet
			.nodes
			.add(root, disabled, node(disabled.0, "DisabledMouth", false));
		add_mesh_drawable(&mut puppet, enabled);
		add_mesh_drawable(&mut puppet, disabled);

		puppet.init_transforms();
		puppet.init_rendering();

		let render_ctx = puppet.render_ctx.as_ref().unwrap();
		assert_eq!(render_ctx.vertex_buffers.verts.len(), 7);
		assert_eq!(render_ctx.vertex_buffers.indices.len(), 9);
		assert!(puppet.node_comps.get::<TexturedMeshRenderCtx>(enabled).is_some());
		assert!(puppet.node_comps.get::<TexturedMeshRenderCtx>(disabled).is_none());
	}
}

/// Same as the reference Inochi2D implementation, Inox2D also aims for a "bring your own rendering backend" design.
/// A custom backend shall implement this trait.
///
/// It is perfectly fine that the trait implementation does not contain everything needed to display a puppet as:
/// - The renderer may not be directly rendering to the screen for flexibility.
/// - The renderer may want platform-specific optimizations, e.g. batching, and the provided implementation is merely for collecting puppet info.
/// - The renderer may be a debug/just-for-fun renderer intercepting draw calls for other purposes.
///
/// Either way, the point is Inox2D will implement a `draw()` method for any `impl InoxRenderer`, dispatching calls based on puppet structure according to Inochi2D standard.
pub trait InoxRenderer {
	/// Begin masking.
	///
	/// Ref impl: Clear and start writing to the stencil buffer, lock the color buffer.
	fn on_begin_masks(&self, masks: &Masks);
	/// Get prepared for rendering a singular Mask.
	fn on_begin_mask(&self, mask: &Mask);
	/// Get prepared for rendering masked content.
	///
	/// Ref impl: Read only from the stencil buffer, unlock the color buffer.
	fn on_begin_masked_content(&self);
	/// End masking.
	///
	/// Ref impl: Disable the stencil buffer.
	fn on_end_mask(&self);

	/// Draw TexturedMesh content.
	// TODO: TexturedMesh without any texture (usually for mesh masks)?
	fn draw_textured_mesh_content(
		&self,
		as_mask: bool,
		components: &TexturedMeshComponents,
		render_ctx: &TexturedMeshRenderCtx,
		id: InoxNodeUuid,
	);

	/// Begin compositing. Get prepared for rendering children of a Composite.
	///
	/// Ref impl: Prepare composite buffers.
	fn begin_composite_content(
		&self,
		as_mask: bool,
		components: &CompositeComponents,
		render_ctx: &CompositeRenderCtx,
		id: InoxNodeUuid,
	);
	/// End compositing.
	///
	/// Ref impl: Transfer content from composite buffers to normal buffers.
	fn finish_composite_content(
		&self,
		as_mask: bool,
		components: &CompositeComponents,
		render_ctx: &CompositeRenderCtx,
		id: InoxNodeUuid,
	);
}

pub trait InoxRendererExt {
	/// Draw a Drawable, which is potentially masked.
	fn draw_drawable(&self, as_mask: bool, comps: &World, id: InoxNodeUuid);

	/// Draw one composite. `components` must be referencing `comps`.
	fn draw_composite(&self, as_mask: bool, comps: &World, components: &CompositeComponents, id: InoxNodeUuid);

	/// Iterate over top-level drawables (excluding masks) in zsort order,
	/// and make draw calls correspondingly.
	///
	/// This effectively draws the complete puppet.
	fn draw(&self, puppet: &Puppet);
}

impl<T: InoxRenderer> InoxRendererExt for T {
	fn draw_drawable(&self, as_mask: bool, comps: &World, id: InoxNodeUuid) {
		let drawable_kind = DrawableKind::new(id, comps, false).expect("Node must be a Drawable.");
		let masks = match drawable_kind {
			DrawableKind::TexturedMesh(ref components) => &components.drawable.masks,
			DrawableKind::Composite(ref components) => &components.drawable.masks,
		};

		let mut has_masks = false;
		if let Some(ref masks) = masks {
			has_masks = true;
			self.on_begin_masks(masks);
			for mask in &masks.masks {
				self.on_begin_mask(mask);

				self.draw_drawable(true, comps, mask.source);
			}
			self.on_begin_masked_content();
		}

		match drawable_kind {
			DrawableKind::TexturedMesh(ref components) => {
				self.draw_textured_mesh_content(as_mask, components, comps.get(id).unwrap(), id)
			}
			DrawableKind::Composite(ref components) => self.draw_composite(as_mask, comps, components, id),
		}

		if has_masks {
			self.on_end_mask();
		}
	}

	fn draw_composite(&self, as_mask: bool, comps: &World, components: &CompositeComponents, id: InoxNodeUuid) {
		let render_ctx = comps.get::<CompositeRenderCtx>(id).unwrap();
		if render_ctx.zsorted_children_list.is_empty() {
			// Optimization: Nothing to be drawn, skip context switching
			return;
		}

		self.begin_composite_content(as_mask, components, render_ctx, id);

		for uuid in &render_ctx.zsorted_children_list {
			let drawable_kind = DrawableKind::new(*uuid, comps, false)
				.expect("All children in zsorted_children_list should be a Drawable.");
			match drawable_kind {
				DrawableKind::TexturedMesh(components) => {
					self.draw_textured_mesh_content(as_mask, &components, comps.get(*uuid).unwrap(), *uuid)
				}
				DrawableKind::Composite { .. } => panic!("Composite inside Composite not allowed."),
			}
		}

		self.finish_composite_content(as_mask, components, render_ctx, id);
	}

	/// Dispatches draw calls for all nodes of `puppet`
	/// - with provided renderer implementation,
	/// - in Inochi2D standard defined order.
	///
	/// This does not guarantee the display of a puppet on screen due to these possible reasons:
	/// - Only provided `InoxRenderer` method implementations are called.
	///
	/// For example, maybe the caller still need to transfer content from a texture buffer to the screen surface buffer.
	/// - The provided `InoxRender` implementation is wrong.
	/// - `puppet` here does not belong to the `model` this `renderer` is initialized with. This will likely result in panics for non-existent node uuids.
	fn draw(&self, puppet: &Puppet) {
		for uuid in &puppet
			.render_ctx
			.as_ref()
			.expect("RenderCtx of puppet must be initialized before calling draw().")
			.root_drawables_zsorted
		{
			self.draw_drawable(false, &puppet.node_comps, *uuid);
		}
	}
}
