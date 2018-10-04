extern crate meshlite;

use meshlite::mesh::Mesh;
use meshlite::subdivide::Subdivide;
use std::time::{Duration, Instant};
use std::vec::Vec;

fn main() {
    let mut mesh = cube();
    let mut all_vps = Vec::new(); // Vertices Per Second
    for _ in 0..9 {
        let now = Instant::now();
        let verts_before = mesh.vertex_count;
        mesh = mesh.subdivide();
        let verts_after = mesh.vertex_count;
        let added_verts = verts_after - verts_before;
        let seconds = to_seconds_f64(&now.elapsed());
        let verts_per_second = (added_verts as f64 / seconds).round();
        all_vps.push(verts_per_second);
        println!(
            concat!(
                "subdivided to {:#?} faces, {} vertices, ",
                "{} vertices/second, time {:.2} ms"
            ),
            mesh.face_count,
            verts_after,
            verts_per_second,
            seconds * 1000.0
        );
    }
    println!(
        "Vertices per second, min: {}, max: {}, avg: {}",
        all_vps.iter().cloned().fold(0. / 0., f64::min),
        all_vps.iter().cloned().fold(0. / 0., f64::max),
        (all_vps.iter().sum::<f64>() / all_vps.len() as f64).round()
    );
}

fn to_seconds_f64(d: &Duration) -> f64 {
    d.as_secs() as f64 + d.subsec_nanos() as f64 * 1e-9
}

fn cube() -> Mesh {
    let mut m = Mesh::new();
    let face_id = m.add_plane(1.0, 1.0);
    let normal = m.face_norm(face_id);
    m.extrude_face(face_id, normal, 1.0)
        .translate(0.0, 0.0, -0.5);
    m
}