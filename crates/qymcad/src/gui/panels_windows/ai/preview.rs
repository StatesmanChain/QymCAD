//! An orbitable preview using the viewport's per-pixel depth rasterizer.
use qymcad_core::{
    geom::{Mesh, Point3},
    model::Project,
};

struct Vertex {
    screen: egui::Pos2,
    depth: f64,
}
#[derive(Clone, PartialEq)]
struct Frame {
    document: u64,
    angle: f64,
    size: [usize; 2],
}
#[derive(Clone)]
struct Cached {
    frame: Frame,
    texture: egui::TextureHandle,
}
pub(super) fn draw(ui: &mut egui::Ui, project: &Project) {
    let consumed = project.consumed_bodies();
    let Some(body) = project.bodies.iter().find(|b| !consumed.contains(&b.id)) else { return };
    if body.mesh.verts.is_empty() {
        return;
    }
    let (response, painter) = ui.allocate_painter(egui::vec2(ui.available_width(), 220.0), egui::Sense::drag());
    let key = response.id.with("orbit");
    let mut angle = ui.ctx().data(|d| d.get_temp::<f64>(key)).unwrap_or(0.65);
    if response.dragged() {
        angle += f64::from(ui.input(|i| i.pointer.delta().x)) * 0.01;
    }
    ui.ctx().data_mut(|d| d.insert_temp(key, angle));
    let ppp = ui.ctx().pixels_per_point().clamp(1.0, 3.0).min(2400.0 / response.rect.width().max(1.0));
    let frame = Frame { document: project.state_key(), angle, size: [(response.rect.width() * ppp).round().max(1.0) as usize, (response.rect.height() * ppp).round().max(1.0) as usize] };
    let cache_key = response.id.with("depth_preview");
    let mut cached = ui.ctx().data(|d| d.get_temp::<Cached>(cache_key));
    if cached.as_ref().is_none_or(|c| c.frame != frame) {
        let image = raster(&body.mesh, &frame);
        if let Some(c) = &mut cached {
            c.texture.set(image, egui::TextureOptions::LINEAR);
            c.frame = frame;
        } else {
            cached = Some(Cached { frame, texture: ui.ctx().load_texture("ai-preview", image, egui::TextureOptions::LINEAR) });
        }
    }
    if let Some(cached) = cached {
        painter.image(cached.texture.id(), response.rect, egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)), egui::Color32::WHITE);
        ui.ctx().data_mut(|d| d.insert_temp(cache_key, cached));
    }
    ui.small(crate::i18n::tr("ai-preview-drag"));
}
fn raster(mesh: &Mesh, frame: &Frame) -> egui::ColorImage {
    let project_vertex = |p: &Point3| {
        let (s, c) = frame.angle.sin_cos();
        let x = c * p.x - s * p.y;
        let y = s * p.x + c * p.y;
        Vertex { screen: egui::pos2(x as f32, (0.55 * y - 0.84 * p.z) as f32), depth: 0.84 * y + 0.55 * p.z }
    };
    let points: Vec<Vertex> = mesh.verts.iter().map(project_vertex).collect();
    let mut bounds = egui::Rect::NOTHING;
    for v in &points {
        bounds.extend_with(v.screen);
    }
    let size = egui::vec2(frame.size[0] as f32, frame.size[1] as f32);
    let margin = 24.0 * size.y / 220.0;
    let scale = ((size.x - margin) / bounds.width().max(1.0)).min((size.y - margin) / bounds.height().max(1.0));
    let screen = |v: &Vertex| egui::Pos2::ZERO + size * 0.5 + (v.screen - bounds.center()) * scale;
    let mut triangles = Vec::with_capacity(mesh.tris.len());
    for indices in &mesh.tris {
        let [a, b, c] = indices.map(|v| mesh.verts[v as usize]);
        let u = Point3::new(b.x - a.x, b.y - a.y, b.z - a.z);
        let v = Point3::new(c.x - a.x, c.y - a.y, c.z - a.z);
        let normal = Point3::new(u.y * v.z - u.z * v.y, u.z * v.x - u.x * v.z, u.x * v.y - u.y * v.x);
        let length = normal.x.hypot(normal.y).hypot(normal.z).max(1e-12);
        let light = ((-0.4 * normal.x + 0.3 * normal.y + 0.8 * normal.z) / length).max(0.0);
        let shade = (65.0 * light) as u8;
        let color = egui::Color32::from_rgb(65 + shade, 120 + shade, 155 + shade);
        triangles.push(qymcad_render::RasterTri {
            v: indices.map(|index| {
                let vertex = &points[index as usize];
                let p = screen(vertex);
                [p.x, p.y, -vertex.depth as f32]
            }),
            cols: [color; 3],
        });
    }
    let mut image = egui::ColorImage::filled(frame.size, egui::Color32::TRANSPARENT);
    let mut depth = vec![f32::INFINITY; image.pixels.len()];
    qymcad_render::raster_band(&mut image.pixels, &mut depth, frame.size[0], 0, frame.size[1], &triangles);
    image
}
