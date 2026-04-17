use std::collections::HashMap;

use indextree::Arena;

use crate::node::{InoxNode, InoxNodeUuid};

pub struct InoxNodeTree {
	// make this public, instead of replicating all node methods for root, now that callers have the root id
	pub root_node_id: InoxNodeUuid,
	arena: Arena<InoxNode>,
	node_ids: HashMap<InoxNodeUuid, indextree::NodeId>,
}

impl InoxNodeTree {
	pub fn new_with_root(node: InoxNode) -> Self {
		let id = node.uuid;
		let mut node_ids = HashMap::new();
		let mut arena = Arena::new();

		let root_id = arena.new_node(node);
		node_ids.insert(id, root_id);

		Self {
			root_node_id: id,
			arena,
			node_ids,
		}
	}

	pub fn add(&mut self, parent: InoxNodeUuid, id: InoxNodeUuid, node: InoxNode) {
		let parent_id = self.node_ids.get(&parent).expect("parent should be added earlier");

		let node_id = self.arena.new_node(node);
		parent_id.append(node_id, &mut self.arena);

		let result = self.node_ids.insert(id, node_id);
		if result.is_some() {
			panic!("duplicate inox node uuid")
		}
	}

	fn get_internal_node(&self, id: InoxNodeUuid) -> Option<&indextree::Node<InoxNode>> {
		self.arena.get(*self.node_ids.get(&id)?)
	}

	fn get_internal_node_mut(&mut self, id: InoxNodeUuid) -> Option<&mut indextree::Node<InoxNode>> {
		self.arena.get_mut(*self.node_ids.get(&id)?)
	}

	pub fn get_node(&self, id: InoxNodeUuid) -> Option<&InoxNode> {
		Some(self.get_internal_node(id)?.get())
	}

	pub fn find_node_by_name(&self, name: &str) -> Option<InoxNodeUuid> {
		self.pre_order_iter()
			.find(|node| node.name == name)
			.map(|node| node.uuid)
	}

	pub fn find_first_node_by_names(&self, names: &[&str]) -> Option<InoxNodeUuid> {
		names.iter().find_map(|name| self.find_node_by_name(name))
	}

	pub fn find_nodes_by_names(&self, names: &[&str]) -> Vec<InoxNodeUuid> {
		self.pre_order_iter()
			.filter(|node| names.iter().any(|name| node.name == *name))
			.map(|node| node.uuid)
			.collect()
	}

	pub fn get_node_mut(&mut self, id: InoxNodeUuid) -> Option<&mut InoxNode> {
		Some(self.get_internal_node_mut(id)?.get_mut())
	}

	/// order is not guaranteed. use pre_order_iter() for pre-order traversal
	pub fn iter(&self) -> impl Iterator<Item = &InoxNode> {
		self.arena.iter().map(|n| {
			if n.is_removed() {
				panic!("There is a removed node inside the indextree::Arena of the node tree.")
			}
			n.get()
		})
	}

	pub fn pre_order_iter(&self) -> impl Iterator<Item = &InoxNode> {
		let root_id = self.node_ids.get(&self.root_node_id).unwrap();
		root_id
			.descendants(&self.arena)
			.map(|id| self.arena.get(id).unwrap().get())
	}

	/// WARNING: panicks if called on root
	pub fn get_parent(&self, children: InoxNodeUuid) -> &InoxNode {
		self.arena
			.get(
				self.arena
					.get(*self.node_ids.get(&children).unwrap())
					.unwrap()
					.parent()
					.unwrap(),
			)
			.unwrap()
			.get()
	}

	pub fn get_children(&self, parent: InoxNodeUuid) -> impl Iterator<Item = &InoxNode> {
		self.node_ids
			.get(&parent)
			.unwrap()
			.children(&self.arena)
			.map(|id| self.arena.get(id).unwrap().get())
	}
}

#[cfg(test)]
mod tests {
	use super::InoxNodeTree;
	use crate::math::transform::TransformOffset;
	use crate::node::{InoxNode, InoxNodeUuid};

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

	#[test]
	fn find_first_node_by_names_respects_candidate_order() {
		let mut tree = InoxNodeTree::new_with_root(node(1, "Root"));
		tree.add(InoxNodeUuid(1), InoxNodeUuid(2), node(2, "Neck"));
		tree.add(InoxNodeUuid(2), InoxNodeUuid(3), node(3, "Face"));

		assert!(tree.find_first_node_by_names(&["Head", "Neck", "Face"]) == Some(InoxNodeUuid(2)));
		assert!(tree.find_first_node_by_names(&["Head"]).is_none());
	}

	#[test]
	fn find_nodes_by_names_collects_all_matching_nodes() {
		let mut tree = InoxNodeTree::new_with_root(node(1, "Root"));
		tree.add(InoxNodeUuid(1), InoxNodeUuid(2), node(2, "Neck"));
		tree.add(InoxNodeUuid(2), InoxNodeUuid(3), node(3, "Face"));
		tree.add(InoxNodeUuid(3), InoxNodeUuid(4), node(4, "Neck"));

		assert!(tree.find_nodes_by_names(&["Neck", "Face"]) == vec![InoxNodeUuid(2), InoxNodeUuid(3), InoxNodeUuid(4)]);
	}
}
