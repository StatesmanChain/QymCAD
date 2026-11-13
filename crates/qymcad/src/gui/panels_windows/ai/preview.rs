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
    for i in faces {
        let vertices = mesh.tris[i].map(|v| screen(&points[v as usize]));
        let shade = ((i % 7) as u8) * 4;
        painter.add(egui::Shape::convex_polygon(vertices.to_vec(), egui::Color32::from_rgb(100 + shade, 160 + shade, 190 + shade), egui::Stroke::NONE));
    }
    ui.small(crate::i18n::tr("ai-preview-drag"));
}
