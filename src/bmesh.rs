use cgmath::Point3;
use cgmath::Vector3;
use cgmath::prelude::*;
use cgmath::Matrix4;
use mesh::Mesh;
use mesh::Id;
use petgraph::prelude::*;
use petgraph::Graph;
use std::collections::HashMap;
use std::collections::HashSet;
use wrap::GiftWrapper;
use triangulate::Triangulate;
use util::*;
use iterator::FaceIterator;
use iterator::FaceHalfedgeIterator;

struct Node {
    radius: f32,
    position: Point3<f32>,
    generated: bool,
    triangle_ring_resolved: bool,
    quad_ring_resolved: bool,
}

struct Edge {
    cuts: Vec<(Vec<Id>, Vector3<f32>)>,
}

struct Ring {
    pub key: Vec<NodeIndex>,
    pub nodes: Vec<NodeIndex>
}

impl Ring {
    pub fn new(first: NodeIndex, second: NodeIndex, third: NodeIndex, fourth: NodeIndex) -> Self {
        let mut nodes = Vec::new();
        nodes.push(first);
        nodes.push(second);
        nodes.push(third);
        if fourth != NodeIndex::end() {
            nodes.push(fourth);
        }
        let mut key = nodes.clone();
        key.sort();
        Ring {
            key: key,
            nodes: nodes,
        }
    }
}

pub struct Bmesh {
    graph : Graph<Node, Edge, Undirected>,
    mesh: Mesh,
    resolve_ring_map: HashSet<Vec<NodeIndex>>,
    resolve_ring_list: Vec<Vec<NodeIndex>>,
    wrap_error_count: i32,
    default_root_node_id: usize,
}

impl Bmesh {
    pub fn new() -> Self {
        Bmesh {
            graph: Graph::new_undirected(),
            mesh: Mesh::new(),
            resolve_ring_map: HashSet::new(),
            resolve_ring_list: Vec::new(),
            wrap_error_count: 0,
            default_root_node_id: 0,
        }
    }

    pub fn add_node(&mut self, position: Point3<f32>, radius: f32) -> usize {
        let node = Node::new(radius, position);
        let node_id = self.graph.add_node(node).index();
        if 0 == self.default_root_node_id {
            self.default_root_node_id = node_id;
        }
        node_id
    }

    pub fn add_edge(&mut self, first_node_id: usize, second_node_id: usize) -> usize {
        let edge = Edge::new();
        self.graph.add_edge(NodeIndex::new(first_node_id), NodeIndex::new(second_node_id), edge).index()
    }

    fn direct_of_nodes(&self, first_node_index: NodeIndex, second_node_index: NodeIndex) -> Vector3<f32> {
        let first_node = self.graph.node_weight(first_node_index).unwrap();
        let second_node = self.graph.node_weight(second_node_index).unwrap();
        let direct = second_node.position - first_node.position;
        direct.normalize()
    }

    fn make_cut(&self, position: Point3<f32>, direct: Vector3<f32>, radius: f32) -> Vec<Point3<f32>> {
        const WORLD_Y_AXIS : Vector3<f32> = Vector3 {x: 0.0, y: 1.0, z: 0.0};
        const WORLD_X_AXIS : Vector3<f32> = Vector3 {x: 1.0, y: 0.0, z: 0.0};
        let mut u = {
            if direct.dot(WORLD_X_AXIS).abs() > 0.707 {
                // horizontal
                direct.cross(WORLD_Y_AXIS)
            } else {
                // vertical
                direct.cross(WORLD_X_AXIS)
            }
        };
        let mut v = u.cross(direct);
        let u = u.normalize() * radius;
        let v = v.normalize() * radius;
        let origin = position + direct * radius;
        let mut f = vec![origin - u - v,
            origin + u - v,
            origin + u + v,
            origin - u + v];
        f
    }

    fn resolve_triangle_ring_from_node(&mut self, node_index: NodeIndex) {
        if self.graph.node_weight(node_index).unwrap().triangle_ring_resolved {
            return;
        }
        self.graph.node_weight_mut(node_index).unwrap().triangle_ring_resolved = true;
        let mut other_node_indices : Vec<NodeIndex> = Vec::new();
        {
            let neighbors = self.graph.neighbors_undirected(node_index);
            for other_index in neighbors.clone() {
                other_node_indices.push(other_index);
            }
        }
        for i in 0..other_node_indices.len() {
            let first_index = other_node_indices[i];
            for j in i + 1..other_node_indices.len() {
                let second_index = other_node_indices[j];
                    if !self.graph.find_edge_undirected(first_index, second_index).is_none() {
                        // found triangle: first_index, node_index, second_index
                        let ring = Ring::new(first_index, node_index, second_index, NodeIndex::end());
                        if !self.resolve_ring_map.contains(&ring.key) {
                            self.resolve_ring_map.insert(ring.key);
                            self.resolve_ring_list.push(ring.nodes);
                        }
                    }
            }
        }
        for other_index in other_node_indices {
            self.resolve_triangle_ring_from_node(other_index);
        }
    }

    fn resolve_quad_ring_from_node(&mut self, node_index: NodeIndex) {
        if self.graph.node_weight(node_index).unwrap().quad_ring_resolved {
            return;
        }
        self.graph.node_weight_mut(node_index).unwrap().quad_ring_resolved = true;
        let mut other_node_indices : Vec<NodeIndex> = Vec::new();
        {
            let neighbors = self.graph.neighbors_undirected(node_index);
            for other_index in neighbors.clone() {
                other_node_indices.push(other_index);
            }
        }
        for i in 0..other_node_indices.len() {
            let first_index = other_node_indices[i];
            for j in i + 1..other_node_indices.len() {
                let second_index = other_node_indices[j];
                    if self.graph.find_edge_undirected(first_index, second_index).is_none() {
                        let mut found_index = NodeIndex::end();
                        for third_index in self.graph.neighbors_undirected(first_index) {
                            if third_index != node_index && !self.graph.find_edge_undirected(third_index, second_index).is_none() {
                                found_index = third_index;
                                break;
                            }
                        }
                        if found_index != NodeIndex::end() {
                            // found quad: first_index, node_index, second_index, found_index
                            let ring = Ring::new(first_index, node_index, second_index, found_index);
                            if !self.resolve_ring_map.contains(&ring.key) &&
                                    !self.resolve_ring_map.contains(&Ring::new(first_index, node_index, second_index, NodeIndex::end()).key) &&
                                    !self.resolve_ring_map.contains(&Ring::new(node_index, second_index, found_index, NodeIndex::end()).key) &&
                                    !self.resolve_ring_map.contains(&Ring::new(second_index, found_index, first_index, NodeIndex::end()).key) &&
                                    !self.resolve_ring_map.contains(&Ring::new(found_index, first_index, node_index, NodeIndex::end()).key) {
                                self.resolve_ring_map.insert(ring.key);
                                self.resolve_ring_list.push(ring.nodes);
                            }
                        }
                    }
            }
        }
        for other_index in other_node_indices {
            self.resolve_quad_ring_from_node(other_index);
        }
    }

    fn get_ring_center(&self, ring: &Vec<NodeIndex>) -> Point3<f32> {
        let mut positions = Vec::new();
        for &node_index in ring {
            positions.push(self.graph.node_weight(node_index).unwrap().position);
        }
        Point3::centroid(&positions)
    }

    fn do_resolve_ring(&mut self, ring: Vec<NodeIndex>) {
        let mut shared_indices = HashSet::new();
        let center = self.get_ring_center(&ring);
        for i in 0..ring.len() {
            let first_node = ring[i];
            let second_node = ring[(i + 1) % ring.len()];
            let edge_index = self.graph.find_edge_undirected(first_node, second_node).unwrap().0;
            let ref edge = self.graph.edge_weight(edge_index).unwrap();
            for cut in &edge.cuts {
                for &vert_id in cut.0.iter() {
                    shared_indices.insert(vert_id);
                }
            }
        }
        let mut remove_face_id_list : Vec<Id> = Vec::new();
        for face_id in FaceIterator::new(&self.mesh) {
            let mut need_remove = true;
            let mut any_point_on_plane = Point3{x:0.0, y:0.0, z:0.0};
            for halfedge_id in FaceHalfedgeIterator::new(&self.mesh, self.mesh.face_first_halfedge_id(face_id).unwrap()) {
                let vert_id = self.mesh.halfedge_start_vertex_id(halfedge_id).unwrap();
                if !shared_indices.contains(&vert_id) {
                    need_remove = false;
                    break;
                }
                any_point_on_plane = self.mesh.vertex(vert_id).unwrap().position;
            }
            if need_remove {
                need_remove = PointSide::Front == point_side_on_plane(center, any_point_on_plane, self.mesh.face_norm(face_id));
            }
            if need_remove {
                remove_face_id_list.push(face_id);
            }
        }
        for face_id in remove_face_id_list {
            self.mesh.remove_face(face_id);
        }
        let mut linked_vertices : HashMap<Id, Id> = HashMap::new();
        for face_id in FaceIterator::new(&self.mesh) {
            for halfedge_id in FaceHalfedgeIterator::new(&self.mesh, self.mesh.face_first_halfedge_id(face_id).unwrap()) {
                if self.mesh.halfedge_opposite_face_id(halfedge_id).is_some() {
                    continue;
                }
                let vert_id = self.mesh.halfedge_start_vertex_id(halfedge_id).unwrap();
                let next_vert_id = self.mesh.halfedge_start_vertex_id(self.mesh.halfedge_next_id(halfedge_id).unwrap()).unwrap();
                if shared_indices.contains(&vert_id) && shared_indices.contains(&next_vert_id) {
                    linked_vertices.entry(next_vert_id).or_insert(vert_id);
                    println!("link {:?} -> {:?}", next_vert_id, vert_id);
                }
            }
        }
        while self.mesh.add_linked_vertices(&mut linked_vertices) > 0 {};
    }

    fn resolve_ring_from_node(&mut self, node_index: NodeIndex) {
        {
            self.resolve_triangle_ring_from_node(node_index);
            self.resolve_quad_ring_from_node(node_index);
        }
        for ring in self.resolve_ring_list.clone() {
            self.do_resolve_ring(ring);
        }
    }

    fn generate_from_node(&mut self, node_index: NodeIndex) {
        if self.graph.node_weight(node_index).unwrap().generated {
            return;
        }
        self.graph.node_weight_mut(node_index).unwrap().generated = true;
        let node_position = self.graph.node_weight(node_index).unwrap().position;
        let node_radius = self.graph.node_weight(node_index).unwrap().radius;
        let mut new_cuts : Vec<(EdgeIndex, (Vec<Id>, Vector3<f32>))> = Vec::new();
        let mut other_node_indices : Vec<NodeIndex> = Vec::new();
        {
            let neighbors = self.graph.neighbors_undirected(node_index);
            let mut neighbors_count = 0;
            let mut directs = Vec::new();
            let mut rev_directs = Vec::new();
            for other_index in neighbors.clone() {
                let direct = self.direct_of_nodes(node_index, other_index);
                directs.push(direct);
                rev_directs.push(-direct);
                other_node_indices.push(other_index);
                neighbors_count += 1;
            }
            if neighbors_count == 1 {
                let direct = directs[0];
                let face = self.make_cut(node_position - direct * node_radius, direct, node_radius);
                let mut vert_ids = Vec::new();
                for vert in face {
                    vert_ids.push(self.mesh.add_vertex(vert));
                }
                for other_index in neighbors.clone() {
                    let edge_index = self.graph.find_edge(node_index, other_index).unwrap();
                    new_cuts.push((edge_index, (vert_ids.clone(), -direct)));
                }
                self.mesh.add_vertices(vert_ids);
            } else if neighbors_count == 2 {
                let mut order = 0;
                let direct = (directs[0] - directs[1]) / 2.0;
                let face = self.make_cut(node_position - direct * node_radius, direct, node_radius);
                let mut vert_ids = Vec::new();
                for vert in face {
                    vert_ids.push(self.mesh.add_vertex(vert));
                }
                let mut rev_vert_ids = vert_ids.clone();
                rev_vert_ids.reverse();
                let cut_faces = vec![vert_ids, rev_vert_ids];
                let cut_directs = vec![direct, -direct];
                for other_index in neighbors.clone() {
                    let edge_index = self.graph.find_edge(node_index, other_index).unwrap();
                    new_cuts.push((edge_index, (cut_faces[order].clone(), -cut_directs[order])));
                    order += 1;
                }
            } else if neighbors_count >= 3 {
                let mut cuts : Vec<(Vec<Point3<f32>>, EdgeIndex, NodeIndex, Vector3<f32>)> = Vec::new();
                let max_round : usize = 25;
                let factor_step = 1.0 / max_round as f32;
                const DIRECT_INITIAL_AFFECT_FACTOR : f32 = 0.5;
                for round in 0..max_round {
                    for other_index in neighbors.clone() {
                        let factor = factor_step * round as f32;
                        let mut direct = self.direct_of_nodes(node_index, other_index);
                        let mut ave_direct = direct;
                        for &rev_direct in rev_directs.iter() {
                            ave_direct += rev_direct;
                        }
                        ave_direct = ave_direct / (rev_directs.len() - 1) as f32;
                        let mut direct_affect_factor = DIRECT_INITIAL_AFFECT_FACTOR;
                        let mut create_radius = node_radius;
                        let mut create_origin = node_position;
                        if round > 0 {
                            direct_affect_factor += (1.0 - DIRECT_INITIAL_AFFECT_FACTOR) * factor;
                            create_radius = node_radius * (1.0 - factor);
                        }
                        direct = (ave_direct * direct_affect_factor + direct * (1.0 - direct_affect_factor)).normalize();
                        if round > 0 {
                            create_origin += direct * (node_radius * factor);
                        }
                        let edge_index = self.graph.find_edge(node_index, other_index).unwrap();
                        println!("round: {:?} other_index:{:?} r:{:?} direct:{:?}", round, other_index, create_radius, direct);
                        let face = self.make_cut(create_origin, direct, create_radius);
                        cuts.push((face, edge_index, other_index, direct));
                    }
                    let wrap_ok = {
                        // test wrap
                        let mut added_loops : Vec<(Vec<Id>, Vector3<f32>)> = Vec::new();
                        let mut test_mesh = self.mesh.clone();
                        for (face, edge_index, other_index, direct) in cuts.clone() {
                            let mut vert_ids = Vec::new();
                            for vert in face {
                                vert_ids.push(test_mesh.add_vertex(vert));
                            }
                            let mut rev_vert_ids = vert_ids.clone();
                            rev_vert_ids.reverse();
                            test_mesh.add_vertices(vert_ids);
                            added_loops.push((rev_vert_ids, direct));
                        }
                        if added_loops.len() > 1 {
                            let mut wrapper = GiftWrapper::new();
                            wrapper.wrap_vertices(&mut test_mesh, &added_loops);
                            wrapper.finished() && test_mesh.broken_face_set().is_empty() && test_mesh.triangulate().is_triangulated_mesh_manifold()
                        } else {
                            false
                        }
                    };
                    if wrap_ok {
                        break;
                    }
                    cuts = Vec::new();
                }
                if cuts.len() > 0 {
                    // real wrap
                    let mut added_loops : Vec<(Vec<Id>, Vector3<f32>)> = Vec::new();
                    for (face, edge_index, other_index, direct) in cuts {
                        let mut vert_ids = Vec::new();
                        for vert in face {
                            vert_ids.push(self.mesh.add_vertex(vert));
                        }
                        let mut rev_vert_ids = vert_ids.clone();
                        rev_vert_ids.reverse();
                        added_loops.push((rev_vert_ids, direct));
                        new_cuts.push((edge_index, (vert_ids, -direct)));
                    }
                    if added_loops.len() > 1 {
                        let mut wrapper = GiftWrapper::new();
                        wrapper.wrap_vertices(&mut self.mesh, &added_loops);
                        wrapper.finished();
                    }
                } else {
                    self.wrap_error_count += 1;
                }
            }
        }
        for (edge_index, cut) in new_cuts {
            let ref mut edge = self.graph.edge_weight_mut(edge_index).unwrap();
            edge.cuts.push(cut);
        }
        for other_index in other_node_indices {
            self.generate_from_node(other_index);
        }
    }

    fn stitch_by_edges(&mut self) {
        for edge in self.graph.edge_weights_mut() {
            match edge.cuts.len() {
                2 => {
                    let mut wrapper = GiftWrapper::new();
                    wrapper.wrap_vertices(&mut self.mesh, &edge.cuts);
                },
                _ => {}
            }
        }
    }

    pub fn generate_mesh(&mut self, root: usize) -> &mut Mesh {
        let root_node_id = {
            if root > 0 {
                root
            } else {
                self.default_root_node_id
            }
        };
        if root_node_id > 0 {
            let root_node = NodeIndex::new(root_node_id);
            self.generate_from_node(root_node);
            if 0 == self.wrap_error_count {
                self.stitch_by_edges();
                self.resolve_ring_from_node(root_node);
            }
        }
        &mut self.mesh
    }
}

impl Node {
    fn new(radius: f32, position: Point3<f32>) -> Self {
        Node {
            radius: radius,
            position: position,
            generated: false,
            triangle_ring_resolved: false,
            quad_ring_resolved: false,
        }
    }
}

impl Edge {
    fn new() -> Self {
        Edge {
            cuts: Vec::new(),
        }
    }
}
