//! An orbitable preview independent of the document viewport.
use qymcad_core::{geom::Point3, model::Project};
struct Vertex {
    screen: egui::Pos2,
    depth: f64,
}
pub(super) fn draw(ui: &mut egui::Ui, project: &Project) {
    let consumed = project.consumed_bodies();
    let Some(body) = project.bodies.iter().find(|b| !consumed.contains(&b.id)) else { return };
    let mesh = &body.mesh;
    if mesh.verts.is_empty() {
        return;
    }
    let (response, painter) = ui.allocate_painter(egui::vec2(ui.available_width(), 220.0), egui::Sense::drag());
    let key = response.id.with("orbit");
    let mut angle = ui.ctx().data(|d| d.get_temp::<f64>(key)).unwrap_or(0.65);
    if response.dragged() {
        angle += f64::from(ui.input(|i| i.pointer.delta().x)) * 0.01;
    }
    ui.ctx().data_mut(|d| d.insert_temp(key, angle));
    let project_vertex = |p: &Point3| {
        let (s, c) = angle.sin_cos();
        let x = c * p.x - s * p.y;
        let y = s * p.x + c * p.y;
        Vertex { screen: egui::pos2(x as f32, (0.55 * y - 0.84 * p.z) as f32), depth: 0.84 * y + 0.55 * p.z }
    };
    let points: Vec<Vertex> = mesh.verts.iter().map(project_vertex).collect();
    let mut bounds = egui::Rect::NOTHING;
    for v in &points {
        bounds.extend_with(v.screen);
    }
    let scale = ((response.rect.width() - 24.0) / bounds.width().max(1.0)).min((response.rect.height() - 24.0) / bounds.height().max(1.0));
    let screen = |v: &Vertex| response.rect.center() + (v.screen - bounds.center()) * scale;
    let mut faces: Vec<usize> = (0..mesh.tris.len()).collect();
    let depth = |i: usize| mesh.tris[i].iter().map(|v| points[*v as usize].depth).sum::<f64>();
    faces.sort_by(|a, b| depth(*a).total_cmp(&depth(*b)));
    let mut painted = egui::Mesh::default();
    for i in faces {
        let vertices = mesh.tris[i].map(|v| screen(&points[v as usize]));
        let [a, b, c] = mesh.tris[i].map(|v| mesh.verts[v as usize]);
        let u = Point3::new(b.x - a.x, b.y - a.y, b.z - a.z);
        let v = Point3::new(c.x - a.x, c.y - a.y, c.z - a.z);
        let normal = Point3::new(u.y * v.z - u.z * v.y, u.z * v.x - u.x * v.z, u.x * v.y - u.y * v.x);
        let length = normal.x.hypot(normal.y).hypot(normal.z).max(1e-12);
        let light = ((-0.4 * normal.x + 0.3 * normal.y + 0.8 * normal.z) / length).max(0.0);
        let shade = (65.0 * light) as u8;
        let color = egui::Color32::from_rgb(65 + shade, 120 + shade, 155 + shade);
        let first = painted.vertices.len() as u32;
        for pos in vertices {
            painted.vertices.push(egui::epaint::Vertex { pos, uv: egui::epaint::WHITE_UV, color });
        }
        painted.add_triangle(first, first + 1, first + 2);
    }
    painter.add(egui::Shape::mesh(painted));
    ui.small(crate::i18n::tr("ai-preview-drag"));
}
