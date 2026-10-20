//! Spotify OAuth (Authorization Code + PKCE) with a local redirect listener.
//! One token serves both the Web API (browse/search) and librespot (streaming).

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Read,
    path::PathBuf,
    process::{Command, Stdio},
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::Mutex,
};

const REDIRECT_URI: &str = "http://127.0.0.1:8898/login";
const REDIRECT_ADDR: &str = "127.0.0.1:8898";
const TOKEN_URL: &str = "https://accounts.spotify.com/api/token";

#[derive(Serialize, Deserialize, Clone)]
struct Stored {
    client_id: String,
    #[serde(default)]
    scopes: String,
    access_token: String,
    refresh_token: String,
    expires_at: u64,
}

#[derive(Deserialize)]
struct TokenResp {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: u64,
}

pub struct Auth {
    client_id: String,
    path: PathBuf,
    http: reqwest::Client,
    st: Mutex<Stored>,
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn token_path(name: &str) -> PathBuf {
    crate::config::dir().join(format!("token_{name}.json"))
}

fn save(st: &Stored, path: &PathBuf) {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let _ = fs::create_dir_all(crate::config::dir());
    if let Ok(s) = serde_json::to_string(st) {
        if let Ok(mut f) = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)
        {
            let _ = f.write_all(s.as_bytes());
        }
    }
}

fn random_bytes(n: usize) -> Vec<u8> {
    let mut buf = vec![0u8; n];
    if let Ok(mut f) = fs::File::open("/dev/urandom") {
        let _ = f.read_exact(&mut buf);
    }
    buf
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn pct(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

async fn parse_token(resp: reqwest::Response) -> Result<TokenResp, String> {
    let status = resp.status();
    let body = resp.text().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        let short: String = body.chars().take(200).collect();
        return Err(format!("token endpoint {}: {}", status.as_u16(), short));
    }
    serde_json::from_str(&body).map_err(|e| format!("bad token response: {e}"))
}

async fn respond(sock: &mut TcpStream, status: &str, body: &str) {
    let html = format!(
        "<html><body style=\"background:#031010;color:#6effdc;font-family:monospace;padding:3em\">\
         <h2>VFD&middot;TUNE</h2><p>{body}</p></body></html>"
    );
    let resp = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{html}",
        html.len()
    );
    let _ = sock.write_all(resp.as_bytes()).await;
    let _ = sock.shutdown().await;
}

async fn wait_for_code(listener: &TcpListener, state: &str) -> Result<String, String> {
    let fut = async {
        loop {
            let (mut sock, _) = listener.accept().await.map_err(|e| e.to_string())?;
            let mut buf = vec![0u8; 8192];
            let n = sock.read(&mut buf).await.map_err(|e| e.to_string())?;
            let req = String::from_utf8_lossy(&buf[..n]).to_string();
            let target = req
                .lines()
                .next()
                .and_then(|l| l.split_whitespace().nth(1))
                .unwrap_or("")
                .to_string();
            let (path, query) = target.split_once('?').unwrap_or((target.as_str(), ""));
            if path != "/login" {
                respond(&mut sock, "404 Not Found", "not found").await;
                continue;
            }
            let (mut code, mut got_state, mut err) = (None, None, None);
            for kv in query.split('&') {
                if let Some((k, v)) = kv.split_once('=') {
                    match k {
                        "code" => code = Some(v.to_string()),
                        "state" => got_state = Some(v.to_string()),
                        "error" => err = Some(v.to_string()),
                        _ => {}
                    }
                }
            }
            if let Some(e) = err {
                respond(&mut sock, "200 OK", "Login was denied. You can close this tab.").await;
                return Err(format!("Spotify denied the login: {e}"));
            }
            match code {
                Some(c) if got_state.as_deref() == Some(state) => {
                    respond(&mut sock, "200 OK", "Logged in. You can close this tab and go back to the terminal.").await;
                    return Ok::<String, String>(c);
                }
                _ => {
                    respond(&mut sock, "400 Bad Request", "bad request").await;
                    continue;
                }
            }
        }
    };
    tokio::time::timeout(Duration::from_secs(180), fut)
        .await
        .map_err(|_| "login timed out after 3 minutes".to_string())?
}

async fn login(
    http: &reqwest::Client,
    client_id: &str,
    scopes: &str,
    label: &str,
) -> Result<Stored, String> {
    let verifier = URL_SAFE_NO_PAD.encode(random_bytes(64));
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let state = hex(&random_bytes(12));
    let url = format!(
        "https://accounts.spotify.com/authorize?response_type=code&client_id={}&scope={}&redirect_uri={}&code_challenge_method=S256&code_challenge={}&state={}",
        pct(client_id),
        pct(scopes),
        pct(REDIRECT_URI),
        challenge,
        state
    );
    let listener = TcpListener::bind(REDIRECT_ADDR)
        .await
        .map_err(|e| format!("cannot listen on {REDIRECT_ADDR} (is another login running?): {e}"))?;

    println!("VFDTUNE {label}: opening your browser to log in to Spotify...");
    println!("If nothing opens, visit this URL:\n\n{url}\n");
    let _ = Command::new("xdg-open")
        .arg(&url)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();

    let code = wait_for_code(&listener, &state).await?;
    let resp = http
        .post(TOKEN_URL)
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", code.as_str()),
            ("redirect_uri", REDIRECT_URI),
            ("client_id", client_id),
            ("code_verifier", verifier.as_str()),
        ])
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let t = parse_token(resp).await?;
    Ok(Stored {
        client_id: client_id.to_string(),
        scopes: scopes.to_string(),
        access_token: t.access_token,
        refresh_token: t.refresh_token.ok_or("Spotify returned no refresh token")?,
        expires_at: now() + t.expires_in,
    })
}

impl Auth {
    /// Use the cached login if it still works, otherwise run the browser flow.
    pub async fn init(
        name: &str,
        label: &str,
        client_id: String,
        scopes: &str,
    ) -> Result<Arc<Auth>, String> {
        let path = token_path(name);
        let http = reqwest::Client::builder()
            .user_agent("vfdtune/0.1")
            .build()
            .map_err(|e| e.to_string())?;

        let cached = fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str::<Stored>(&s).ok());
        if let Some(stored) = cached {
            if stored.client_id == client_id && stored.scopes == scopes {
                let auth = Arc::new(Auth {
                    client_id: client_id.clone(),
                    path: path.clone(),
                    http: http.clone(),
                    st: Mutex::new(stored),
                });
                if auth.force_refresh().await.is_ok() {
                    return Ok(auth);
                }
            }
        }

        let st = login(&http, &client_id, scopes, label).await?;
        save(&st, &path);
        Ok(Arc::new(Auth {
            client_id,
            path,
            http,
            st: Mutex::new(st),
        }))
    }

    async fn refresh_locked(&self, st: &mut Stored) -> Result<String, String> {
        let resp = self
            .http
            .post(TOKEN_URL)
            .form(&[
                ("grant_type", "refresh_token"),
                ("refresh_token", st.refresh_token.as_str()),
                ("client_id", self.client_id.as_str()),
            ])
            .send()
            .await
            .map_err(|e| e.to_string())?;
        let t = parse_token(resp).await?;
        st.access_token = t.access_token;
        if let Some(r) = t.refresh_token {
            st.refresh_token = r;
        }
        st.expires_at = now() + t.expires_in;
        save(st, &self.path);
        Ok(st.access_token.clone())
    }

    pub async fn force_refresh(&self) -> Result<String, String> {
        let mut st = self.st.lock().await;
        self.refresh_locked(&mut st).await
    }

    pub async fn access_token(&self) -> Result<String, String> {
        let mut st = self.st.lock().await;
        if st.expires_at > now() + 60 {
            return Ok(st.access_token.clone());
        }
        self.refresh_locked(&mut st).await
    }
}
