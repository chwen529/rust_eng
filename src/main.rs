use eng::{load_decks, Card, Deck};
use std::env;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

const INDEX: &str = include_str!("../static/index.html");

struct App {
    dir: PathBuf,
    decks: Mutex<Vec<Deck>>,
}

fn main() {
    let mut dir = PathBuf::from(r"D:\eng");
    let mut port = 8741u16;
    let mut open = true;
    let args: Vec<String> = env::args().skip(1).collect();
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--dir" => {
                index += 1;
                dir = PathBuf::from(args.get(index).map(String::as_str).unwrap_or(""));
            }
            "--port" => {
                index += 1;
                port = args
                    .get(index)
                    .and_then(|value| value.parse().ok())
                    .unwrap_or(8741);
            }
            "--no-open" => open = false,
            "--help" | "-h" => {
                println!("用法: eng [--dir D:\\eng] [--port 8741] [--no-open]");
                return;
            }
            other => {
                eprintln!("不認識的參數：{other}");
                eprintln!("用法: eng [--dir D:\\eng] [--port 8741] [--no-open]");
                std::process::exit(1);
            }
        }
        index += 1;
    }

    let decks = match load_decks(&dir) {
        Ok(decks) => decks,
        Err(error) => {
            eprintln!("讀不到教材資料夾 {}：{error}", dir.display());
            std::process::exit(1);
        }
    };
    if decks.is_empty() {
        eprintln!("{} 裡沒有讀到單字。", dir.display());
        std::process::exit(1);
    }
    let total: usize = decks.iter().map(|deck| deck.cards.len()).sum();
    let app = Arc::new(App {
        dir,
        decks: Mutex::new(decks),
    });

    let (listener, port) = match bind_port(port) {
        Some(bound) => bound,
        None => {
            eprintln!("連接埠 {port} 起的 20 個埠都無法使用。");
            std::process::exit(1);
        }
    };
    let url = format!("http://127.0.0.1:{port}");
    println!("英文學習  {url}");
    println!("教材  {}", app.dir.display());
    println!("{total} 張卡片");
    println!("關掉這個視窗就會停止。");

    if open {
        let _ = Command::new("cmd").args(["/C", "start", "", &url]).spawn();
    }

    for connection in listener.incoming() {
        let Ok(stream) = connection else {
            continue;
        };
        let app = Arc::clone(&app);
        thread::spawn(move || {
            let _ = handle(stream, &app);
        });
    }
}

fn bind_port(start: u16) -> Option<(TcpListener, u16)> {
    for port in start..start.saturating_add(20) {
        if let Ok(listener) = TcpListener::bind(("127.0.0.1", port)) {
            return Some((listener, port));
        }
    }
    None
}

fn handle(mut stream: TcpStream, app: &App) -> std::io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(8)))?;
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 2048];
    loop {
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(size) => {
                buffer.extend_from_slice(&chunk[..size]);
                if buffer.windows(4).any(|window| window == b"\r\n\r\n") || buffer.len() > 65_536 {
                    break;
                }
            }
            Err(error)
                if error.kind() == std::io::ErrorKind::TimedOut
                    || error.kind() == std::io::ErrorKind::WouldBlock =>
            {
                break;
            }
            Err(error) => return Err(error),
        }
    }
    let request = String::from_utf8_lossy(&buffer);
    let path = request
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or("/");
    let (status, content_type, body) = route(path, app);
    let header = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(header.as_bytes())?;
    stream.write_all(&body)?;
    Ok(())
}

fn route(path: &str, app: &App) -> (String, &'static str, Vec<u8>) {
    let (route, query) = path.split_once('?').unwrap_or((path, ""));
    match route {
        "/" | "/index.html" => (
            "200 OK".to_string(),
            "text/html; charset=utf-8",
            INDEX.as_bytes().to_vec(),
        ),
        "/favicon.ico" => (
            "204 No Content".to_string(),
            "text/plain; charset=utf-8",
            Vec::new(),
        ),
        "/api/decks" => (
            "200 OK".to_string(),
            "application/json; charset=utf-8",
            decks_json(app).into_bytes(),
        ),
        "/api/cards" => (
            "200 OK".to_string(),
            "application/json; charset=utf-8",
            cards_json(app, query_value(query, "deck").as_deref()).into_bytes(),
        ),
        "/api/speak" => match speak_wav(
            &query_value(query, "text").unwrap_or_default(),
            query_value(query, "speed")
                .and_then(|value| value.parse().ok())
                .unwrap_or(140),
        ) {
            Ok(wav) => ("200 OK".to_string(), "audio/wav", wav),
            Err(error) => (
                "503 Service Unavailable".to_string(),
                "text/plain; charset=utf-8",
                error.into_bytes(),
            ),
        },
        "/api/reload" => match reload(app) {
            Ok(()) => (
                "200 OK".to_string(),
                "application/json; charset=utf-8",
                decks_json(app).into_bytes(),
            ),
            Err(error) => (
                "500 Internal Server Error".to_string(),
                "application/json; charset=utf-8",
                format!("{{\"error\":{}}}", json_string(&error.to_string())).into_bytes(),
            ),
        },
        _ => (
            "404 Not Found".to_string(),
            "text/plain; charset=utf-8",
            "找不到".as_bytes().to_vec(),
        ),
    }
}

fn speak_wav(text: &str, speed: u32) -> Result<Vec<u8>, String> {
    let text: String = text
        .chars()
        .filter(|ch| !ch.is_control())
        .take(240)
        .collect();
    let text = text.trim();
    if text.is_empty() {
        return Err("沒有文字".to_string());
    }
    let exe = espeak_exe().ok_or_else(|| "找不到 eSpeak".to_string())?;
    let speed = speed.clamp(80, 220).to_string();
    let mut command = Command::new(exe);
    command.args(["-v", "en-us", "-s", &speed, "--stdout", "--", text]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let output = command.output().map_err(|error| error.to_string())?;
    if !output.status.success() || !output.stdout.starts_with(b"RIFF") {
        let detail = String::from_utf8_lossy(&output.stderr);
        let detail = detail.trim();
        return Err(if detail.is_empty() {
            "eSpeak 沒有產生語音".to_string()
        } else {
            detail.to_string()
        });
    }
    Ok(output.stdout)
}

fn espeak_exe() -> Option<PathBuf> {
    let mut candidates = vec![
        PathBuf::from(r"C:\Program Files\eSpeak NG\espeak-ng.exe"),
        PathBuf::from(r"C:\Program Files (x86)\eSpeak NG\espeak-ng.exe"),
    ];
    if let Ok(local) = env::var("LOCALAPPDATA") {
        candidates.push(PathBuf::from(local).join(r"Microsoft\WinGet\Links\espeak-ng.exe"));
    }
    if let Some(path) = candidates.into_iter().find(|path| path.is_file()) {
        return Some(path);
    }
    let output = Command::new("where.exe")
        .arg("espeak-ng.exe")
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && Path::new(line).is_file())
        .map(PathBuf::from)
}

fn reload(app: &App) -> std::io::Result<()> {
    let decks = load_decks(&app.dir)?;
    *app.decks.lock().expect("deck lock") = decks;
    Ok(())
}

fn decks_json(app: &App) -> String {
    let decks = app.decks.lock().expect("deck lock");
    let mut out = String::from("{\"dir\":");
    out.push_str(&json_string(&app.dir.display().to_string()));
    out.push_str(",\"decks\":[");
    for (index, deck) in decks.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str("{\"name\":");
        out.push_str(&json_string(&deck.name));
        out.push_str(",\"count\":");
        out.push_str(&deck.cards.len().to_string());
        out.push('}');
    }
    out.push_str("]}");
    out
}

fn cards_json(app: &App, deck_name: Option<&str>) -> String {
    let decks = app.decks.lock().expect("deck lock");
    let mut out = String::from("{\"cards\":[");
    let mut first = true;
    for deck in selected(&decks, deck_name) {
        for card in &deck.cards {
            if !first {
                out.push(',');
            }
            first = false;
            push_card(&mut out, card);
        }
    }
    out.push_str("]}");
    out
}

fn selected<'a>(decks: &'a [Deck], deck_name: Option<&str>) -> Vec<&'a Deck> {
    match deck_name {
        None | Some("*") | Some("") => decks.iter().collect(),
        Some(name) => decks.iter().filter(|deck| deck.name == name).collect(),
    }
}

fn push_card(out: &mut String, card: &Card) {
    out.push('{');
    field(out, "english", &card.english, true);
    field(out, "syllables", &card.syllables, false);
    field(out, "hint", &card.hint, false);
    field(out, "meaning", &card.meaning, false);
    field(out, "note", &card.note, false);
    field(out, "section", &card.section, false);
    field(out, "source", &card.source, false);
    out.push('}');
}

fn field(out: &mut String, name: &str, value: &str, first: bool) {
    if !first {
        out.push(',');
    }
    out.push_str(&json_string(name));
    out.push(':');
    out.push_str(&json_string(value));
}

fn query_value(query: &str, name: &str) -> Option<String> {
    for pair in query.split('&') {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        if percent_decode(key) == name {
            return Some(percent_decode(value));
        }
    }
    None
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let Ok(byte) = u8::from_str_radix(&value[index + 1..index + 3], 16) {
                out.push(byte);
                index += 3;
                continue;
            }
        }
        if bytes[index] == b'+' {
            out.push(b' ');
        } else {
            out.push(bytes[index]);
        }
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn json_string(value: &str) -> String {
    let mut out = String::from("\"");
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if (ch as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", ch as u32)),
            ch => out.push(ch),
        }
    }
    out.push('"');
    out
}
