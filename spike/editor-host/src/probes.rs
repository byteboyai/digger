//! 行为探针(`SELFTEST=policy`):在真实 WKWebView 里验证 `hosts.rs` 钉死的设置确实生效。
//! 编辑器/预览探针页经 IPC 汇报;应用探针页(不信任,IPC 只认 nonce 白名单)经本机小服务器汇报。

use crate::hosts::{Class, Recorder};
use std::io::{Read, Write};
use std::net::TcpListener;
use tokio::sync::mpsc::UnboundedSender;

pub struct Probe {
    pub tag: &'static str,
    pub class: Class,
    /// 数据存储标识所用的应用 id(应用探针)。
    pub store_id: &'static str,
    /// 页面模式:full = 全部拒绝项 + 写 localStorage;read = 只读 localStorage。
    pub mode: &'static str,
}

pub const PROBES: &[Probe] = &[
    Probe { tag: "editor", class: Class::Editor, store_id: "", mode: "" },
    Probe { tag: "preview", class: Class::Preview, store_id: "", mode: "" },
    Probe { tag: "app-a", class: Class::App, store_id: "policy-a", mode: "full" },
    Probe { tag: "app-b", class: Class::App, store_id: "policy-b", mode: "read" },
    Probe { tag: "app-c", class: Class::App, store_id: "policy-a", mode: "read" },
];

pub const EDITOR_PROBE_HTML: &str = r#"<!doctype html><meta charset="utf-8"><body>probe
<script>
const post=(k,v)=>window.ipc.postMessage('probe:'+k+':'+v);
const sleep=ms=>new Promise(r=>setTimeout(r,ms));
(async()=>{
  const w=window.open('https://example.com/'); post('open_blocked', String(w===null));
  try{ await fetch('digger://x/'); post('custom_proto','reachable'); }catch(e){ post('custom_proto','blocked'); }
  const a=document.createElement('a'); a.href=URL.createObjectURL(new Blob(['x'])); a.download='x.txt';
  document.body.appendChild(a); a.click();
  await sleep(600);
  const before=location.href; location.assign('https://example.com/'); await sleep(800);
  post('nav_stayed', String(location.href===before));
  post('has_ipc', String(typeof window.ipc==='object'));
  post('done','1');
})();
</script>"#;

const APP_PAGE: &str = r#"<!doctype html><meta charset="utf-8"><body>app
<script>
const q=new URLSearchParams(location.search), T=q.get('t'), M=q.get('m');
const rep=(k,v)=>fetch('/__report?t='+encodeURIComponent(T)+'&k='+encodeURIComponent(k)+'&v='+encodeURIComponent(v));
const sleep=ms=>new Promise(r=>setTimeout(r,ms));
(async()=>{
  if(M==='read'){ await rep('ls', String(localStorage.getItem('k'))); await rep('done','1'); return; }
  localStorage.setItem('k','written-by-'+T); await rep('ls_written', localStorage.getItem('k'));
  const w=window.open('https://example.com/'); await rep('open_blocked', String(w===null));
  try{ await fetch('digger://x/'); await rep('custom_proto','reachable'); }catch(e){ await rep('custom_proto','blocked'); }
  const a=document.createElement('a'); a.href=URL.createObjectURL(new Blob(['x'])); a.download='x.txt';
  document.body.appendChild(a); a.click(); await sleep(600);
  // The page tries to talk to the host: no nonce / guessed nonce / synthetic (untrusted) user event.
  window.ipc.postMessage('focus'); window.ipc.postMessage('deadbeef:focus');
  document.dispatchEvent(new MouseEvent('mousedown',{bubbles:true})); await sleep(300);
  const where=location.href;
  for (const u of ['https://example.com/','http://policy-evil.localhost:'+location.port+'/','file:///etc/hosts','data:text/html,x']) {
    location.assign(u); await sleep(600);
  }
  await rep('nav_stayed', String(location.href===where));
  location.assign('/other?t='+encodeURIComponent(T)); // same origin: allowed
})();
</script>"#;

const OTHER_PAGE: &str = r#"<!doctype html><meta charset="utf-8"><body>other
<script>
const T=new URLSearchParams(location.search).get('t');
const rep=(k,v)=>fetch('/__report?t='+encodeURIComponent(T)+'&k='+k+'&v='+v);
rep('other_loaded','1').then(()=>rep('done','1'));
</script>"#;

/// 本机小 HTTP 服务器(任何 Host 都响应),返回端口。`/__report` 把探针页的汇报转发进主循环。
pub fn start_server(forward: UnboundedSender<String>) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind probe server");
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let fwd = forward.clone();
            std::thread::spawn(move || {
                let mut stream = stream;
                let mut buf = vec![0u8; 8192];
                let n = stream.read(&mut buf).unwrap_or(0);
                let head = String::from_utf8_lossy(&buf[..n]);
                let target = head.split_whitespace().nth(1).unwrap_or("/").to_string();
                let url = url::Url::parse(&format!("http://x{target}")).unwrap();
                let (status, body) = match url.path() {
                    "/" => ("200 OK", APP_PAGE.to_string()),
                    "/other" => ("200 OK", OTHER_PAGE.to_string()),
                    "/__report" => {
                        let g = |k: &str| url.query_pairs().find(|(a, _)| a == k).map(|(_, v)| v.into_owned()).unwrap_or_default();
                        let _ = fwd.send(format!("{}|page|{}={}", g("t"), g("k"), g("v")));
                        ("200 OK", String::new())
                    }
                    _ => ("404 Not Found", String::new()),
                };
                let _ = write!(
                    stream,
                    "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
            });
        }
    });
    port
}

/// 把 `tag|ipc|probe:k:v` 或 `tag|page|k=v` 解析成 (tag, k, v)。
pub fn parse_event(raw: &str) -> Option<(String, String, String)> {
    let mut it = raw.splitn(3, '|');
    let (tag, kind, rest) = (it.next()?, it.next()?, it.next()?);
    match kind {
        "ipc" => {
            let r = rest.strip_prefix("probe:")?;
            let (k, v) = r.split_once(':')?;
            Some((tag.into(), k.into(), v.into()))
        }
        "page" => {
            let (k, v) = rest.split_once('=')?;
            Some((tag.into(), k.into(), v.into()))
        }
        _ => None,
    }
}

/// 对一个已汇报完的探针做断言,返回 (名称, 是否通过) 列表。
pub fn evaluate(p: &Probe, got: &std::collections::HashMap<String, String>, rec: &Recorder) -> Vec<(String, bool)> {
    let g = |k: &str| got.get(k).map(String::as_str);
    let navs = rec.navigations.lock().unwrap().clone();
    let denied = |needle: &str| navs.iter().any(|(u, ok)| !ok && u.contains(needle));
    let t = p.tag;
    let mut out: Vec<(String, bool)> = Vec::new();
    let mut c = |name: &str, ok: bool| out.push((format!("{t}: {name}"), ok));
    match (p.class, p.mode) {
        (Class::Editor | Class::Preview, _) => {
            c("window.open is refused (returns null)", g("open_blocked") == Some("true"));
            c("the host was told about the refused new window", rec.new_windows_denied.lock().unwrap().iter().any(|u| u.contains("example.com")));
            c("a download attempt is refused", rec.downloads_denied.load(std::sync::atomic::Ordering::SeqCst) >= 1);
            c("top-level navigation away from the host document is refused", g("nav_stayed") == Some("true") && denied("example.com"));
            c("only the initial document was ever allowed", navs.iter().all(|(u, ok)| *ok == (u == "about:blank")));
            c("no custom protocol is reachable", g("custom_proto") == Some("blocked"));
            c("the Digger bridge (window.ipc) is present", g("has_ipc") == Some("true"));
        }
        (Class::App, "full") => {
            c("window.open is refused (returns null)", g("open_blocked") == Some("true"));
            c("a download attempt is refused", rec.downloads_denied.load(std::sync::atomic::Ordering::SeqCst) >= 1);
            c("no custom protocol is reachable", g("custom_proto") == Some("blocked"));
            c("cross-site navigation is refused", denied("example.com"));
            c("another app's origin (same port) is refused", denied("policy-evil.localhost"));
            // 注意:`data:` 由我们的回调拒绝;`file:///` 在到达回调之前就被 WebKit 自己拦下(http 页面不能跳到 file:),
            // 所以这一项断言"结果"——它们从不生效——而不是要求回调看到过它们。
            c("file: and data: navigations never take effect", navs.iter().all(|(u, ok)| !(*ok && (u.starts_with("file:") || u.starts_with("data:")))));
            c("the page stayed where it was", g("nav_stayed") == Some("true"));
            c("a same-origin navigation is allowed and loads", g("other_loaded") == Some("1") && navs.iter().any(|(u, ok)| *ok && u.contains("/other")));
            c("forged IPC without the nonce is rejected (2 attempts)", rec.ipc_rejected.load(std::sync::atomic::Ordering::SeqCst) >= 2);
            c("synthetic (untrusted) events produce no host message", rec.ipc_accepted.lock().unwrap().is_empty());
            c("localStorage works inside the app", g("ls_written") == Some("written-by-app-a"));
        }
        (Class::App, _) => {
            if p.tag == "app-b" {
                c("a different data-store id does NOT see app-a's localStorage", g("ls") == Some("null"));
            } else {
                c("the same data-store id sees the stored value", g("ls") == Some("written-by-app-a"));
            }
        }
    }
    out
}
