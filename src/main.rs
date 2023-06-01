use actix_web::{
    get, middleware, web, web::BytesMut, App, Error, HttpRequest, HttpResponse, HttpServer,
    Responder,
};
mod raw_guard;

use actix::{
    fut::{future::Map, wrap_stream},
    prelude::*,
    AsyncContext,
};
use actix_web_actors::ws;

use async_stream::stream;
use futures_util::stream::poll_fn;
use log::{error, info, warn};
use mime_guess::from_path;
use pty_process::{OwnedReadPty, OwnedWritePty, Pty, Size};
use serde_json::Value;
use tokio_stream::{wrappers::LinesStream, StreamExt};
use tokio_util::io::ReaderStream;

use std::{collections::HashMap, env, ops::DerefMut, sync::Arc, task::Poll};

use tokio::{
    io::{stdout, AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWriteExt, BufReader},
    sync::Mutex,
};

#[derive(Message)]
#[rtype(result = "Result<(), ()>")]
struct CommandRunner(String);
#[derive(Message)]
#[rtype(result = "Result<(), ()>")]
struct CommandResize(String);

#[derive(Debug, Message)]
#[rtype(result = "Result<(), ()>")]
struct Line(String);

#[derive(Message)]
#[rtype(result = "()")]
struct Ping;
/// Define HTTP actor
struct MyWs {
    // pty: Option<Arc<Mutex<Pty>>>,
    r: Option<Arc<Mutex<OwnedReadPty>>>,
    w: Option<Arc<Mutex<OwnedWritePty>>>,
}
impl MyWs {
    fn new() -> Self {
        MyWs { r: None, w: None }
    }
}

impl Default for MyWs {
    fn default() -> Self {
        Self::new()
    }
}
impl Actor for MyWs {
    type Context = ws::WebsocketContext<Self>;
    fn started(&mut self, ctx: &mut Self::Context) {
        let filtered_env: HashMap<String, String> = env::vars()
            .filter(|&(ref k, _)| k == "TERM" || k == "TZ" || k == "LANG" || k == "PATH")
            .collect();
        let pty = pty_process::Pty::new().unwrap();
        pty.resize(pty_process::Size::new(100, 200)).unwrap();
        let mut cmd = pty_process::Command::new("zsh");
        cmd.envs(filtered_env);

        cmd.env("key", "xterm-256color");
        let child = cmd.spawn(&pty.pts().unwrap()).unwrap();

        // run(&mut child, &mut pty).await.expect("error run");
        let (r, w) = pty.into_split();
        self.r = Some(Arc::new(Mutex::new(r)));
        self.w = Some(Arc::new(Mutex::new(w)));
        let _raw = raw_guard::RawGuard::new();

        // self.pty = Some(Arc::new(Mutex::new(pty)));
        let output = self.r.clone();

        ctx.add_stream(stream! {
            let mut out_buf = [0_u8; 40960];
            loop{
                let mutex = output.clone().expect("clone error");
                {
                    match mutex.try_lock_owned(){

                        Ok(mut lock)=>{
                            info!("read lock");


                        let len =( lock).read(&mut out_buf).await.expect("read error");
                        let s = String::from_utf8(out_buf[..len].to_vec()).expect("cov str error");
                        yield Ok(Line(s));
                        },
                        Err(_err)=>{   print!("stream lock fail")},
                    };
                }
                // let _ = tokio::time::sleep(Duration::from_millis(1000)).await;
                info!("read unlock");

            }
        });
    }
}

impl StreamHandler<Result<Line, ws::ProtocolError>> for MyWs {
    fn handle(&mut self, msg: Result<Line, ws::ProtocolError>, ctx: &mut Self::Context) {
        match msg {
            Ok(line) => ctx.text(line.0),
            Err(err) => (), //Handle errors
        }
    }
}

impl StreamHandler<Ping> for MyWs {
    fn handle(&mut self, item: Ping, ctx: &mut Self::Context) {
        info!("PING");
        // System::current().stop()
    }
    fn finished(&mut self, ctx: &mut Self::Context) {
        info!("finished");
    }
}
/// Handler for ws::Message message
impl StreamHandler<Result<ws::Message, ws::ProtocolError>> for MyWs {
    fn handle(&mut self, msg: Result<ws::Message, ws::ProtocolError>, ctx: &mut Self::Context) {
        match msg {
            Ok(ws::Message::Continuation(_)) => {}
            Ok(ws::Message::Pong(_)) => {}
            Ok(ws::Message::Close(_)) => {}
            Ok(ws::Message::Nop) => {}
            Ok(ws::Message::Ping(msg)) => ctx.pong(&msg),
            Ok(ws::Message::Text(text)) => {
                info!("recevie from ws text: {text}");

                if text.len() > 3 {
                    ctx.notify(CommandResize(text.to_string()));
                    return;
                }
                ctx.notify(CommandRunner(text.to_string()));
            }
            Ok(ws::Message::Binary(bin)) => {
                info!("recevie from ws bin: {bin:?}");
                ctx.binary(bin)
            }
            _ => (),
        }
    }
}

impl Handler<CommandResize> for MyWs {
    type Result = Result<(), ()>;

    fn handle(&mut self, msg: CommandResize, ctx: &mut Self::Context) -> Self::Result {
        let pty = self.w.clone();
        async move {
            let binding = pty.clone().unwrap();
            let lock = binding.lock().await;
            let resize: Value = serde_json::from_str(&msg.0.to_string()).unwrap();
            let resize = resize.get("resize").expect("get resize error");
            let cols = resize.get("cols").expect("get cols fail").as_u64().expect("cols type err");
            let rows = resize.get("rows").expect("get rows fail ").as_u64().expect("rows type err");
            lock.resize(Size::new_u64(rows, cols)).unwrap();
        }
        .into_actor(self)
        .spawn(ctx);
        Ok(())
    }
}

impl Handler<CommandRunner> for MyWs {
    type Result = Result<(), ()>;
    fn handle(&mut self, msg: CommandRunner, ctx: &mut Self::Context) -> Self::Result {
        // let cmd = Arc::new(&msg);
        let pty = self.w.clone();

        let fut = async move {
            let binding = pty.expect("err pty");
            let mut lock = binding.lock().await;
            //  if let Ok(ref mut mutex) = lock {
            info!("write lock");
            let _ = lock
                .write_all(msg.0.to_owned().as_bytes())
                .await
                .expect("write error");
            // let _ = lock.flush().await.expect("flush error");
            // info!("w flush");
            info!("write unlock");

            ();
        };
        fut.into_actor(self).spawn(ctx);
        // let lang_server_fut = actix::fut::wrap_future(fut.into_actor(self).spawn(ctx));
        // ctx.spawn(lang_server_fut);

        Ok(())
    }
}
impl Drop for MyWs {
    fn drop(&mut self) {
        drop(self.w.take());
        drop(self.r.take());
    }
}

async fn ws_handler(req: HttpRequest, stream: web::Payload) -> Result<HttpResponse, Error> {
    let resp = ws::start(MyWs::new(), &req, stream);
    info!("{:?}", resp);
    resp
}
#[derive(rust_embed::RustEmbed)]
#[folder = "web/"]
struct Asset;
fn handle_embedded_file(path: &str) -> HttpResponse {
    match Asset::get(path) {
        Some(content) => HttpResponse::Ok()
            .content_type(from_path(path).first_or_octet_stream().as_ref())
            .body(content.data.into_owned()),
        None => HttpResponse::NotFound().body("404 Not Found"),
    }
}

#[get("/")]
async fn index() -> impl Responder {
    handle_embedded_file("index.html")
}

#[actix_web::get("/{_:.*}")]
async fn staticfs(path: web::Path<String>) -> impl Responder {
    handle_embedded_file(path.as_str())
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    std::env::set_var("RUST_LOG", "error");
    env_logger::init();
    // std::env::set_var("RUST_LOG", "actix_server=info,actix_web=info");
    info!("server start at: http://0.0.0.0:8080");
    HttpServer::new(|| {
        App::new()
            .wrap(middleware::Logger::default())
            .route("/ws", web::get().to(ws_handler))
            .service(index)
            .service(staticfs)
    })
    .bind("0.0.0.0:8080")?
    .run()
    .await
}

pub async fn run(
    child: &mut tokio::process::Child,
    pty: &mut Pty,
) -> std::result::Result<(), Box<dyn std::error::Error>> {
    // let _raw = super::raw_guard::RawGuard::new();
    let mut in_buf = [0_u8; 40960];
    let mut out_buf = [0_u8; 40960];

    let mut stdin = tokio::io::stdin();
    let mut stdout = tokio::io::stdout();

    #[allow(clippy::trivial_regex)]
    let re = regex::bytes::Regex::new("Elbereth").unwrap();

    loop {
        tokio::select! {
            bytes = stdin.read(&mut in_buf) => match bytes {
                Ok(bytes) => {
                    // engrave Elbereth with ^E
                    if in_buf[..bytes].contains(&5u8) {
                        for byte in in_buf[..bytes].iter() {
                            match byte {
                                5u8 => pty
                                    .write_all(b"E-  Elbereth\n")
                                    .await
                                    .unwrap(),
                                _ => pty
                                    .write_all(&[*byte])
                                    .await
                                    .unwrap(),
                            }
                        }
                    } else {

                        pty.write_all(&in_buf[..bytes]).await.unwrap();
                    }
                }
                Err(e) => {
                    error!("stdin read failed: {:?}", e);
                    break;
                }
            },
            bytes = pty.read(&mut out_buf) => match bytes {
                Ok(bytes) => {
                    // highlight successful Elbereths
                    if re.is_match(&out_buf[..bytes]) {
                        stdout.write_all(&out_buf[..bytes]).await.unwrap();

                    } else {
                        stdout.write_all(&out_buf[..bytes]).await.unwrap();
                    }
                    stdout.flush().await.unwrap();
                }
                Err(e) => {
                    error!("pty read failed: {:?}", e);
                    break;
                }
            },
            _ = child.wait() => break,
        }
    }

    Ok(())
}

#[tokio::test]
async fn test_list() {
    let mut p = pty::PsuedoTerminal::allocate();
    let mut command = tokio::process::Command::new("bash");
    command.env("TERM", "xterm-256color");
    let (mut tty, child) = p.unwrap().spawn(command).await.expect("fail to spawn");
    let mut buffer = vec![0u8; 256];
    let pid: u32 = child.id().unwrap_or(0);
    let _ = tty.write(b"htop\n").await;
    let _ = tty.flush().await;
    loop {
        tokio::select! {
            read = tty.read(&mut buffer[..]) => {
                let read = read.map_err(|e| log::error!("Failed to read from child {pid} PTY: {e}")).unwrap();
                // let mut message = std::mem::replace(&mut buffer, vec![0u8; 256]);
                // message[0] = b'd';
                // message.truncate(read );
                let mut stdout=stdout();
                stdout.write_all(&buffer[..read]).await.unwrap();

            },
        }
    }
}

struct T(String);
impl AsyncRead for T {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        let b = buf.initialize_unfilled();
        return std::task::Poll::Ready(Ok(()));
    }
}

#[tokio::test]
async fn test_mutex() {
    let t = T("a".to_string());
    ReaderStream::new(t);

    let t1 = T("a".to_string());
    let at: Arc<Mutex<T>> = Arc::new(Mutex::new(t1));
    let at = at.clone();
    let mut l: tokio::sync::MutexGuard<T> = at.lock().await;
    let dl = l.deref_mut();
    ReaderStream::new(dl);
}
