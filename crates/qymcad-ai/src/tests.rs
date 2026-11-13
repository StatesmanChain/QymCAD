use super::*;
const ANSWER: &str = include_str!("../../../examples/ai/housing.json");
#[test]
fn a_typed_plan_survives_json_and_fences() {
    assert!(parse(ANSWER).unwrap().plan.is_some());
    assert!(parse(&format!("```json\n{ANSWER}\n```")).is_ok());
}
#[test]
fn code_unknown_operations_and_truncation_are_refused() {
    for bad in ["import os", &ANSWER.replace("\"type\":\"box\"", "\"type\":\"execute\""), &ANSWER.replace("\"x\":150", "\"x\":0"), &ANSWER.replace("\"size\":", "\"file\":")] {
        assert!(parse(bad).is_err());
    }
    let reply = serde_json::json!({"choices":[{"finish_reason":"length","message":{"content":ANSWER}}]});
    assert!(parse_envelope(&reply.to_string()).is_err());
}
#[test]
fn unknown_operation_fields_are_refused() {
    assert!(parse(&ANSWER.replace("\"type\":\"box\"", "\"type\":\"box\",\"extra\":1")).is_err());
}
#[test]
fn stored_connection_never_contains_the_key() {
    let c = Connection { endpoint: "http://127.0.0.1:1234/v1/chat/completions".into(), model: "local".into(), key: "private-secret".into() };
    assert!(!serde_json::to_string(&c).unwrap().contains("private-secret"));
    assert!(endpoint(&c.endpoint).is_ok());
    for bad in ["file:///tmp/a", "https://user:password@example.com/v1", "https://example.com/v1?key=secret"] {
        assert!(endpoint(bad).is_err());
    }
}
#[test]
fn local_endpoint_receives_a_request_and_returns_a_recipe() {
    use std::{
        io::{Read, Write},
        net::TcpListener,
    };
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let worker = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let mut bytes = Vec::new();
        loop {
            let mut block = [0u8; 4096];
            let n = stream.read(&mut block).unwrap();
            assert!(n > 0);
            bytes.extend_from_slice(&block[..n]);
            if let Some(end) = bytes.windows(4).position(|x| x == b"\r\n\r\n") {
                let header = String::from_utf8_lossy(&bytes[..end]);
                let size = header.lines().find_map(|l| l.to_lowercase().strip_prefix("content-length: ").and_then(|s| s.parse::<usize>().ok())).unwrap();
                if bytes.len() >= end + 4 + size {
                    break;
                }
            }
        }
        let request = String::from_utf8(bytes).unwrap();
        assert!(request.contains("Make a housing"));
        assert!(request.to_lowercase().contains("authorization: bearer local-secret"));
        let body = serde_json::json!({"choices":[{"finish_reason":"stop","message":{"content":ANSWER}}]}).to_string();
        write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
    });
    let c = Connection { endpoint: format!("http://{address}/v1/chat/completions"), model: "local-model".into(), key: "local-secret".into() };
    assert!(ask(&c, "Make a housing", &[]).unwrap().plan.is_some());
    worker.join().unwrap();
}
