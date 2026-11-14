//! The assistant's complete path through real frames and a local API stand-in.
use qymcad::{Session, Kind, Key, Modifiers, Arg};
use std::{
    io::{Read, Write},
    net::TcpListener,
    time::Duration,
};
fn word(s: &mut Session, key: &str) {
    let text = s.word(key);
    s.press_word(&text);
}
#[derive(Clone, Copy)]
enum Input {
    Text,
    Drawing,
}
#[test]
fn ai_preview_add_undo_and_redo_preserve_the_original_document() {
    exercise(Input::Text);
}
#[test]
fn image_chooser_removal_and_dimensioned_plate_reach_the_document() {
    exercise(Input::Drawing);
}
fn exercise(input: Input) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    listener.set_nonblocking(true).unwrap();
    let server = std::thread::spawn(move || {
        let start = std::time::Instant::now();
        let mut stream = loop {
            match listener.accept() {
                Ok((s, _)) => break s,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(start.elapsed() < Duration::from_secs(90), "the UI sent no API request");
                    std::thread::sleep(Duration::from_millis(20));
                }
                Err(e) => panic!("{e}"),
            }
        };
        stream.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
        let mut bytes = Vec::new();
        loop {
            let mut block = [0u8; 4096];
            let n = stream.read(&mut block).unwrap();
            assert!(n > 0);
            bytes.extend_from_slice(&block[..n]);
            if let Some(end) = bytes.windows(4).position(|x| x == b"\r\n\r\n") {
                let header = String::from_utf8_lossy(&bytes[..end]);
                let size = header.lines().find_map(|s| s.to_lowercase().strip_prefix("content-length: ").and_then(|s| s.parse::<usize>().ok())).unwrap();
                if bytes.len() >= end + 4 + size {
                    break;
                }
            }
        }
        let end = bytes.windows(4).position(|x| x == b"\r\n\r\n").unwrap() + 4;
        let request: serde_json::Value = serde_json::from_slice(&bytes[end..]).unwrap();
        let latest = request["messages"].as_array().unwrap().last().unwrap();
        let content = match input {
            Input::Text => {
                assert!(latest["content"].is_string());
                include_str!("../../../examples/ai/housing.json")
            }
            Input::Drawing => {
                let parts = latest["content"].as_array().unwrap();
                assert_eq!(parts.len(), 2);
                assert_eq!(parts[0]["type"], "text");
                assert!(!parts[0]["text"].as_str().unwrap().is_empty());
                assert_eq!(parts[1]["type"], "image_url");
                assert_eq!(parts[1]["image_url"]["detail"], "high");
                assert!(parts[1]["image_url"]["url"].as_str().unwrap().starts_with("data:image/png;base64,iVBORw0KGgo"));
                assert!(!request.to_string().contains("dimensioned-plate.png"));
                include_str!("../../../examples/ai/plate.json")
            }
        };
        let body = serde_json::json!({"choices":[{"finish_reason":"stop","message":{"content":content}}]}).to_string();
        write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
    });
    let mut s = Session::start();
    s.resize(qymcad::vec2(1500.0, 1200.0));
    s.key(Key::Escape);
    let before = s.document();
    word(&mut s, "menu-windows");
    word(&mut s, "ai-title");
    word(&mut s, "ai-settings");
    s.fill(&s.word("ai-endpoint"), &format!("http://{address}/v1/chat/completions"));
    s.fill(&s.word("ai-model"), "test-model");
    if matches!(input, Input::Text) {
        s.fill(&s.word("ai-prompt"), "Make a housing 150 x 100 x 60 mm, wall and floor 3 mm");
    }
    word(&mut s, "ai-settings");
    if matches!(input, Input::Drawing) {
        word(&mut s, "ai-add-image");
        assert!(s.chooser().is_some());
        s.cancel_file();
        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/ai/dimensioned-plate.png");
        for attempt in 0..2 {
            word(&mut s, "ai-add-image");
            s.answer_file(&fixture);
            s.wait_for("a decoded attachment", Duration::from_secs(15), |s| {
                let label = s.word_with("ai-images-attached", Arg { name: "count", value: "1" });
                s.words().iter().any(|w| w == &label)
            });
            if attempt == 0 {
                let label = s.word_with("ai-images-attached", Arg { name: "count", value: "1" });
                s.press_word(&label);
                assert!(s.words().iter().any(|w| w == "dimensioned-plate.png"));
                word(&mut s, "ai-remove-image");
                let send = s.word("ai-send");
                assert!(!s.widgets().iter().any(|w| w.kind == Kind::Button && w.label == send && w.enabled));
            }
        }
        let remove = s.word("ai-remove-image");
        if s.words().contains(&remove) {
            let label = s.word_with("ai-images-attached", Arg { name: "count", value: "1" });
            s.press_word(&label);
        }
    }
    let send = s.word("ai-send");
    assert!(s.widgets().iter().any(|w| w.kind == Kind::Button && w.label == send && w.enabled), "the completed prompt enables generation: {:?}", s.words());
    word(&mut s, "ai-send");
    let preview = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        s.wait_for("a checked preview", Duration::from_secs(30), |s| {
            let label = s.word("ai-apply");
            s.widgets().iter().any(|w| w.kind == Kind::Button && w.label == label && w.enabled)
        });
    }));
    if let Ok(path) = std::env::var("QYMCAD_AI_SCREENSHOT") {
        let path = std::path::PathBuf::from(path);
        let path = if matches!(input, Input::Drawing) { path.with_file_name("ai-images-preview.png") } else { path };
        std::fs::write(path, s.snapshot().png()).unwrap();
    }
    if let Err(reason) = preview {
        eprintln!("Assistant window after the request: {:?}", s.words());
        std::panic::resume_unwind(reason);
    }
    if matches!(input, Input::Drawing) {
        flat_plate_has_no_occluded_triangles(&s.snapshot());
    }
    assert_eq!(s.document().parts, before.parts, "preview leaves the document alone");
    word(&mut s, "ai-apply");
    let after = s.document();
    assert_eq!(after.parts.len(), before.parts.len() + 1);
    assert_eq!(after.undo.len(), before.undo.len() + 1);
    let name = match input {
        Input::Text => "AI housing",
        Input::Drawing => "AI plate",
    };
    let body = after.bodies.iter().position(|b| b.part.as_deref() == Some(name) && !b.consumed).unwrap();
    let inspection = s.inspect(body).unwrap();
    assert!(inspection.valid);
    assert_eq!(inspection.solids, 1);
    let expected = match input {
        Input::Text => 150.0 * 100.0 * 60.0 - 144.0 * 94.0 * 57.0,
        Input::Drawing => 100.0 * 60.0 * 5.0 - std::f64::consts::PI * 3.0 * 3.0 * 5.0,
    };
    assert!((after.bodies[body].volume - expected).abs() < 0.1);
    let title = s.word("ai-title");
    s.close_window(&title);
    s.chord(Modifiers::COMMAND, Key::Z);
    assert_eq!(s.document().parts, before.parts, "undo removes exactly the AI part");
    s.chord(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z);
    assert_eq!(s.document().parts.len(), after.parts.len());
    assert!(s.id_clashes().is_empty());
    server.join().unwrap();
}

fn flat_plate_has_no_occluded_triangles(picture: &qymcad::Picture) {
    let mut bounds = qymcad::Rect::NOTHING;
    for (index, pixel) in picture.rgba.as_chunks::<4>().0.iter().enumerate() {
        if (65..=130).contains(&pixel[0]) && pixel[1] == pixel[0] + 55 && pixel[2] == pixel[0] + 90 {
            bounds.extend_with(qymcad::pos2((index % picture.width) as f32, (index / picture.width) as f32));
        }
    }
    assert!(bounds.width() > 100.0 && bounds.height() > 50.0, "the preview contains a plate");
    // Inside the flat top face, away from the through hole and the silhouette.
    // Sorting whole triangles painted the underside across this patch of the top.
    let at = bounds.min + bounds.size() * qymcad::vec2(0.43, 0.35);
    let actual = picture.colour_at(at);
    assert!(qymcad::Picture::apart(actual, [117, 172, 207]) <= 1, "the top face has a uniform shade, got {actual:?}");
}
