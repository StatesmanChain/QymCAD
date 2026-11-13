//! The assistant's complete path through real frames and a local API stand-in.
use qymcad::{Session, Kind, Key, Modifiers};
use std::{
    io::{Read, Write},
    net::TcpListener,
    time::Duration,
};
fn word(s: &mut Session, key: &str) {
    let text = s.word(key);
    s.press_word(&text);
}
fn fill(s: &mut Session, index: usize, text: &str) {
    let fields: Vec<_> = s.widgets().into_iter().filter(|w| w.kind == Kind::TextField).collect();
    let field = fields.get(index).unwrap_or_else(|| panic!("field {index} is on screen: {fields:?}"));
    s.click(field.rect.center()).chord(Modifiers::COMMAND, Key::A).type_text(text);
}
#[test]
fn ai_preview_add_undo_and_redo_preserve_the_original_document() {
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
        let content = include_str!("../../../examples/ai/housing.json");
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
    fill(&mut s, 0, &format!("http://{address}/v1/chat/completions"));
    fill(&mut s, 1, "test-model");
    fill(&mut s, 3, "Make a housing 150 x 100 x 60 mm, wall and floor 3 mm");
    word(&mut s, "ai-settings");
    word(&mut s, "ai-send");
    s.wait_for("a checked preview", Duration::from_secs(30), |s| {
        let label = s.word("ai-apply");
        s.widgets().iter().any(|w| w.kind == Kind::Button && w.label == label && w.enabled)
    });
    assert_eq!(s.document().parts, before.parts, "preview leaves the document alone");
    if let Ok(path) = std::env::var("QYMCAD_AI_SCREENSHOT") {
        std::fs::write(path, s.snapshot().png()).unwrap();
    }
    word(&mut s, "ai-apply");
    let after = s.document();
    assert_eq!(after.parts.len(), before.parts.len() + 1);
    assert_eq!(after.undo.len(), before.undo.len() + 1);
    let body = after.bodies.iter().position(|b| b.part.as_deref() == Some("AI housing") && !b.consumed).unwrap();
    let inspection = s.inspect(body).unwrap();
    assert!(inspection.valid);
    assert_eq!(inspection.solids, 1);
    assert!((after.bodies[body].volume - (150.0 * 100.0 * 60.0 - 144.0 * 94.0 * 57.0)).abs() < 0.1);
    let title = s.word("ai-title");
    s.close_window(&title);
    s.chord(Modifiers::COMMAND, Key::Z);
    assert_eq!(s.document().parts, before.parts, "undo removes exactly the AI part");
    s.chord(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z);
    assert_eq!(s.document().parts.len(), after.parts.len());
    assert!(s.id_clashes().is_empty());
    server.join().unwrap();
}
