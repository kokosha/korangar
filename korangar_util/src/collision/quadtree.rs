use std::hash::Hash;
use std::mem::swap;

use cgmath::Point3;

use crate::container::{SecondarySimpleSlab, SimpleKey, SimpleSlab};
use crate::create_simple_key;

create_simple_key!(NodeKey);

use crate::collision::AABB;

/// The QuadTree is uncompacted and objects can be inserted it.
pub struct Uncompacted {
    max_depth: usize,
    spill_size: usize,
}

/// The QuadTree is compacted and ready to be queried.
pub struct Compacted {}

/// Trait that a shape has to implement, so that it can be inserted into a
/// QuadTree.
pub trait Insertable: Copy {
    fn intersects_aabb(&self, aabb: &AABB) -> bool;
    fn max_y(&self) -> f32;
    fn min_y(&self) -> f32;
}

/// Trait that a shape has to implement, so that it can be used to query a
/// QuadTree.
pub trait Query<O> {
    fn intersects_aabb(&self, aabb: &AABB) -> bool;
    fn intersects_object(&self, object: &O) -> bool;
}

/// A simple QuadTree implementation for collision detection for a
/// 3D world with a mainly top-down view. It is assumed that the X/Z plane is
/// the ground plane and the Y dimension is the "height" dimension of the world.
pub struct QuadTree<K, O: Copy, S> {
    nodes: SimpleSlab<NodeKey, QuadTreeNode<K>>,
    objects: SecondarySimpleSlab<K, O>,
    root_node_key: NodeKey,
    state: S,
}

struct QuadTreeNode<K> {
    boundary: AABB,
    children: Option<[NodeKey; 4]>,
    keys: Vec<K>,
}

impl<K: SimpleKey, O: Insertable> QuadTree<K, O, Uncompacted> {
    /// Creates a new QuadTree in which spatial objects can be inserted. Must be
    /// compacted before it can be queried.
    pub fn new(boundary: AABB, max_depth: usize, spill_size: usize) -> QuadTree<K, O, Uncompacted> {
        let mut nodes = SimpleSlab::default();
        let objects = SecondarySimpleSlab::default();

        let root_node = QuadTreeNode {
            boundary,
            children: None,
            keys: vec![],
        };

        let root_node_key = nodes.insert(root_node).expect("node slab is full");

        QuadTree {
            nodes,
            objects,
            root_node_key,
            state: Uncompacted { max_depth, spill_size },
        }
    }
}

impl<K: SimpleKey, O: Insertable> QuadTree<K, O, Uncompacted> {
    /// Compacts the QuadTree, by reducing the heights of each QuadTree node to
    /// the lowest/highest value of its children or objects. After compacting
    /// it, no new objects can be added and the QuadTree is ready for
    /// queries.
    pub fn compact(mut self) -> QuadTree<K, O, Compacted> {
        self.compact_node(self.root_node_key);

        QuadTree {
            nodes: self.nodes,
            objects: self.objects,
            root_node_key: self.root_node_key,
            state: Compacted {},
        }
    }

    fn compact_node(&mut self, node_key: NodeKey) -> (f32, f32) {
        let mut min_y = f32::MAX;
        let mut max_y = f32::MIN;

        let node = self.nodes.get(node_key).expect("can't find node");

        if let Some(children) = node.children {
            let (child_min, child_max) = children
                .iter()
                .copied()
                .map(|child_key| self.compact_node(child_key))
                .fold((f32::MAX, f32::MIN), |(acc_min, acc_max), (child_min, child_max)| {
                    (acc_min.min(child_min), acc_max.max(child_max))
                });
            min_y = min_y.min(child_min);
            max_y = max_y.max(child_max);
        } else if let Some((obj_min, obj_max)) = node
            .keys
            .iter()
            .copied()
            .filter_map(|key| self.objects.get(key))
            .map(|obj| (obj.min_y(), obj.max_y()))
            .fold(None, |acc: Option<(f32, f32)>, (obj_min, obj_max)| {
                Some(match acc {
                    None => (obj_min, obj_max),
                    Some((acc_min, acc_max)) => (acc_min.min(obj_min), acc_max.max(obj_max)),
                })
            })
        {
            min_y = min_y.min(obj_min);
            max_y = max_y.max(obj_max);
        } else {
            // This node has no children or object, it will also not receive new objects,
            // since we compact the tree. We can safely set this node to a flat value.
            min_y = 0.0;
            max_y = 0.0;
        }

        let node = self.nodes.get_mut(node_key).expect("can't find node");
        let min = node.boundary.min();
        let max = node.boundary.max();

        node.boundary = AABB::new(Point3::new(min.x, min_y, min.z), Point3::new(max.x, max_y, max.z));

        (min_y, max_y)
    }

    /// Insert a spatial object into the QuadTree, which can later be queried.
    pub fn insert(&mut self, key: K, object: O) {
        self.objects.insert(key, object);
        self.insert_recursive(self.root_node_key, key, object, 0);
    }

    fn insert_recursive(&mut self, node_key: NodeKey, key: K, object: O, depth: usize) {
        let node = self.nodes.get_mut(node_key).expect("can't find node");

        if !object.intersects_aabb(&node.boundary) {
            return;
        }

        let children = node.children;

        match children {
            None => {
                node.keys.push(key);

                if node.keys.len() > self.state.spill_size && depth < self.state.max_depth {
                    self.split(node_key);
                    self.redistribute_objects(node_key, depth);
                }
            }
            Some(children) => {
                for child_key in children {
                    self.insert_recursive(child_key, key, object, depth + 1);
                }
            }
        }
    }

    fn split(&mut self, node_key: NodeKey) {
        let node = self.nodes.get(node_key).expect("can't find node");

        let min = node.boundary.min();
        let max = node.boundary.max();
        let center = node.boundary.center();

        let child_boundaries = [
            AABB::new(Point3::new(min.x, min.y, min.z), Point3::new(center.x, max.y, center.z)),
            AABB::new(Point3::new(center.x, min.y, min.z), Point3::new(max.x, max.y, center.z)),
            AABB::new(Point3::new(min.x, min.y, center.z), Point3::new(center.x, max.y, max.z)),
            AABB::new(Point3::new(center.x, min.y, center.z), Point3::new(max.x, max.y, max.z)),
        ];

        let child_indices: [NodeKey; 4] = child_boundaries.map(|boundary| {
            self.nodes
                .insert(QuadTreeNode {
                    boundary,
                    keys: Vec::new(),
                    children: None,
                })
                .expect("node slab is full")
        });

        let node = self.nodes.get_mut(node_key).expect("can't find node");
        node.children = Some(child_indices);
    }

    fn redistribute_objects(&mut self, node_key: NodeKey, depth: usize) {
        let node = self.nodes.get_mut(node_key).expect("can't find node");
        let mut keys = Vec::with_capacity(0);
        swap(&mut node.keys, &mut keys);

        if let Some(children) = node.children {
            for key in keys {
                if let Some(object) = self.objects.get(key).copied() {
                    for child_index in children.iter().copied() {
                        self.insert_recursive(child_index, key, object, depth + 1);
                    }
                }
            }
        }
    }
}

impl<K: SimpleKey + Ord, O: Insertable> QuadTree<K, O, Compacted> {
    /// Queries the compacted QuadTree. Returns a list of all keys that
    /// intersected with the given query.
    pub fn query(&self, query: &impl Query<O>, result: &mut Vec<K>) {
        // Broad phase
        self.query_recursive(self.root_node_key, query, result);

        // Because keys are integer values, hashing overhead and cache locality, this is
        // faster than a HashSet for our data sizes (sub 1000 keys after broad
        // phase).
        result.sort_unstable();
        result.dedup_by_key(|k| *k);

        // Near phase
        result.retain(|key| {
            if let Some(object) = self.objects.get(*key) {
                query.intersects_object(object)
            } else {
                false
            }
        });
    }

    fn query_recursive(&self, node_key: NodeKey, query: &impl Query<O>, result: &mut Vec<K>) {
        let node = self.nodes.get(node_key).expect("can't find node");

        if !query.intersects_aabb(&node.boundary) {
            return;
        }

        for key in node.keys.iter().copied() {
            result.push(key);
        }

        if let Some(children) = node.children {
            for &child_key in &children {
                self.query_recursive(child_key, query, result);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use cgmath::Point3;

    use super::*;

    create_simple_key!(TestKey);

    fn generate_objects(slab: &mut SimpleSlab<TestKey, ()>) -> [(TestKey, AABB); 3] {
        [
            (
                slab.insert(()).unwrap(),
                AABB::new(Point3::new(10.0, 0.0, 10.0), Point3::new(20.0, 30.0, 20.0)),
            ),
            (
                slab.insert(()).unwrap(),
                AABB::new(Point3::new(80.0, 0.0, 80.0), Point3::new(90.0, 40.0, 90.0)),
            ),
            (
                slab.insert(()).unwrap(),
                AABB::new(Point3::new(10.0, 0.0, 60.0), Point3::new(20.0, 50.0, 70.0)),
            ),
        ]
    }

    #[test]
    fn test_quadtree_insertion_and_splitting() {
        let root_aabb = AABB::new(Point3::new(0.0, 0.0, 0.0), Point3::new(100.0, 100.0, 100.0));
        let mut tree = QuadTree::new(root_aabb, 2, 2);
        let mut slab = SimpleSlab::default();
        let objects = generate_objects(&mut slab);

        for (key, object) in objects.iter().copied() {
            tree.insert(key, object);
        }

        let root = tree.nodes.get(tree.root_node_key).unwrap();
        assert!(root.children.is_some(), "Root should have children after insertions");
        assert_eq!(root.keys.len(), 0, "Root should have no objects directly");
        assert_eq!(root.boundary.max().y, 100.0, "Root max Y should not be updated");

        if let Some(children) = root.children {
            let ne_child = tree.nodes.get(children[0]).unwrap();
            assert_eq!(ne_child.keys, vec![objects[0].0]);

            let nw_child = tree.nodes.get(children[1]).unwrap();
            assert!(nw_child.keys.is_empty());

            let se_child = tree.nodes.get(children[2]).unwrap();
            assert_eq!(se_child.keys, vec![objects[2].0]);

            let sw_child = tree.nodes.get(children[3]).unwrap();
            assert_eq!(sw_child.keys, vec![objects[1].0]);

            assert!(nw_child.keys.is_empty());
        } else {
            panic!("Root should have children");
        }

        // root + 4 children
        assert_eq!(tree.nodes.count(), 5);
    }

    #[test]
    fn test_quadtree_compaction() {
        let root_aabb = AABB::new(Point3::new(0.0, 0.0, 0.0), Point3::new(100.0, 100.0, 100.0));
        let mut tree = QuadTree::new(root_aabb, 2, 2);
        let mut slab = SimpleSlab::default();
        let objects = generate_objects(&mut slab);

        for (key, object) in objects.iter().copied() {
            tree.insert(key, object);
        }

        let compacted_tree = tree.compact();
        let root = compacted_tree.nodes.get(compacted_tree.root_node_key).unwrap();

        assert_eq!(root.boundary.min().y, 0.0, "Root min Y should remain 0.0");
        assert_eq!(
            root.boundary.max().y,
            50.0,
            "Root max Y should be updated to the highest object"
        );

        if let Some(children) = root.children {
            let ne_child = compacted_tree.nodes.get(children[0]).unwrap();
            assert_eq!(ne_child.boundary.min().y, 0.0, "NE child min Y should be 0.0");
            assert_eq!(ne_child.boundary.max().y, 30.0, "NE child max Y should be 30.0");

            let sw_child = compacted_tree.nodes.get(children[3]).unwrap();
            assert_eq!(sw_child.boundary.min().y, 0.0, "SW child min Y should be 0.0");
            assert_eq!(sw_child.boundary.max().y, 40.0, "SW child max Y should be 40.0");

            let se_child = compacted_tree.nodes.get(children[2]).unwrap();
            assert_eq!(se_child.boundary.min().y, 0.0, "SE child min Y should be 0.0");
            assert_eq!(se_child.boundary.max().y, 50.0, "SE child max Y should be 50.0");

            let nw_child = compacted_tree.nodes.get(children[1]).unwrap();
            assert_eq!(nw_child.boundary.min().y, 0.0, "NW child min Y should be 0.0");
            assert_eq!(nw_child.boundary.max().y, 0.0, "NW child max Y should be 0.0");
        } else {
            panic!("Root should have children");
        }
    }

    #[test]
    fn test_quadtree_query() {
        let root_aabb = AABB::new(Point3::new(0.0, 0.0, 0.0), Point3::new(100.0, 100.0, 100.0));
        let mut tree = QuadTree::new(root_aabb, 2, 2);
        let mut slab = SimpleSlab::default();
        let objects = generate_objects(&mut slab);

        for (key, object) in objects.iter().copied() {
            tree.insert(key, object);
        }
        let compacted_tree = tree.compact();

        let query = AABB::new(Point3::new(0.0, 0.0, 0.0), Point3::new(30.0, 100.0, 100.0));
        let mut results = vec![];
        compacted_tree.query(&query, &mut results);

        assert_eq!(results.len(), 2);
        assert!(results.contains(&objects[0].0));
        assert!(results.contains(&objects[2].0));

        let query = AABB::new(Point3::new(70.0, 0.0, 70.0), Point3::new(100.0, 100.0, 100.0));
        let mut results = vec![];
        compacted_tree.query(&query, &mut results);

        assert_eq!(results.len(), 1);
        assert!(results.contains(&objects[1].0));
    }

    #[test]
    fn test_quadtree_query_distinct() {
        let root_aabb = AABB::new(Point3::new(0.0, 0.0, 0.0), Point3::new(100.0, 100.0, 100.0));
        let mut tree = QuadTree::new(root_aabb, 2, 2);
        let mut slab = SimpleSlab::default();

        let spanning_object = (
            slab.insert(()).unwrap(),
            AABB::new(Point3::new(40.0, 0.0, 40.0), Point3::new(60.0, 30.0, 60.0)),
        );

        tree.insert(spanning_object.0, spanning_object.1);
        let compacted_tree = tree.compact();

        let query = AABB::new(Point3::new(30.0, 0.0, 30.0), Point3::new(70.0, 40.0, 70.0));
        let mut results: Vec<TestKey> = vec![];
        compacted_tree.query(&query, &mut results);

        assert_eq!(results.len(), 1, "Should have no duplicates");
        assert!(results.contains(&spanning_object.0), "Should contain the spanning object");
    }

    #[test]
    fn test_quadtree_max_depth() {
        let root_aabb = AABB::new(Point3::new(0.0, 0.0, 0.0), Point3::new(100.0, 100.0, 100.0));
        let mut tree = QuadTree::new(root_aabb, 1, 1);
        let mut slab: SimpleSlab<TestKey, ()> = SimpleSlab::default();

        for _ in 0..5 {
            let key = slab.insert(()).unwrap();
            let object = AABB::new(Point3::new(10.0, 0.0, 10.0), Point3::new(20.0, 30.0, 20.0));
            tree.insert(key, object);
        }

        let root = tree.nodes.get(tree.root_node_key).unwrap();
        assert!(root.children.is_some(), "Root should have children");

        if let Some(children) = root.children {
            for child_key in children.iter() {
                let child = tree.nodes.get(*child_key).unwrap();
                assert!(child.children.is_none(), "Child nodes should not have further subdivisions");
            }
        }
    }

    #[test]
    fn test_empty_quadtree_query() {
        let root_aabb = AABB::new(Point3::new(0.0, 0.0, 0.0), Point3::new(100.0, 100.0, 100.0));
        let tree = QuadTree::new(root_aabb, 2, 2).compact();

        let query = AABB::new(Point3::new(0.0, 0.0, 0.0), Point3::new(100.0, 100.0, 100.0));
        let mut results: Vec<TestKey> = vec![];
        tree.query(&query, &mut results);

        assert!(results.is_empty(), "Query on empty tree should return no results");
    }
}
