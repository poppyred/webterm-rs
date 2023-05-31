#![feature(mutex_unlock)]

use actix_web::web::BytesMut;
use actix_web::{
    get, middleware, web, App, Error, HttpRequest, HttpResponse, HttpServer, Responder,
};

use actix::prelude::*;
use actix::AsyncContext;
use actix_web_actors::ws;

use async_stream::stream;
use mime_guess::from_path;
use pty_process::{OwnedReadPty, OwnedWritePty, Pty};
use std::collections::HashMap;
use std::env;
use std::io::Read;
use tokio::io::AsyncWriteExt;
use tokio::io::{stdout, AsyncReadExt};
use tokio::sync::Mutex;
use tokio_util::io::CopyToBytes;

use std::sync::Arc;

#[derive(Message)]
#[rtype(result = "Result<(), ()>")]
struct CommandRunner(String);

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
        pty.resize(pty_process::Size::new(50, 130)).unwrap();
        let mut cmd = pty_process::Command::new("sh");
        cmd.envs(filtered_env);

        cmd.env("key", "xterm-256color");
        let child = cmd.spawn(&pty.pts().unwrap()).unwrap();
        // run(&mut child, &mut pty).await.expect("error run");
        let (r, w) = pty.into_split();
        self.r = Some(Arc::new(Mutex::new(r)));
        self.w = Some(Arc::new(Mutex::new(w)));
        // self.pty = Some(Arc::new(Mutex::new(pty)));
        // let r = self.r.clone().unwrap();
        // let lines = tokio::io::BufReader::new( {
        //    r
        // }).lines();
        // ctx.add_stream(LinesStream::new(lines).map(|l|{
        //     Ok(Line(String::from(l.unwrap())))
        // }));
        let output = self.r.clone();
        ctx.add_stream(stream! {
            let mut out_buf = [0_u8; 40960];
            loop{
                let mutex = output.clone().expect("clone error");
                {
                    match mutex.try_lock_owned(){
                        Ok(mut lock)=>{

                        let len =( lock).read(&mut out_buf).await.expect("read error");
                        let s = String::from_utf8(out_buf[..len].to_vec()).expect("cov str error");
                        yield Ok(Line(s));
                        },
                        Err(_err)=>{   print!("stream lock fail")},
                    };
                }
                // let _ = tokio::time::sleep(Duration::from_millis(1000)).await;
                println!("sleep");

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

impl StreamHandler<Result<BytesMut, std::io::Error>> for MyWs {
    fn handle(&mut self, msg: Result<BytesMut, std::io::Error>, ctx: &mut Self::Context) {
        match msg {
            Ok(line) => {
                println!("{line:?}");
                ctx.binary(line)
            }
            Err(err) => (), //Handle errors
        }
    }
}

impl StreamHandler<Line> for MyWs {
    fn handle(&mut self, msg: Line, ctx: &mut Self::Context) {
        println!("{}", msg.0);
        ctx.text(msg.0);
    }
}

impl StreamHandler<Ping> for MyWs {
    fn handle(&mut self, item: Ping, ctx: &mut Self::Context) {
        println!("PING");
        // System::current().stop()
    }

    fn finished(&mut self, ctx: &mut Self::Context) {
        println!("finished");
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
                println!("recevie from ws text: {text}");
                ctx.notify(CommandRunner(text.to_string()));
            }
            Ok(ws::Message::Binary(bin)) => {
                println!("recevie from ws bin: {bin:?}");
                ctx.binary(bin)
            }
            _ => (),
        }
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

            let _ = lock
                .write_all(msg.0.to_owned().as_bytes())
                .await
                .expect("write error");
            let _ = lock.flush().await.expect("flush error");
            println!("w flush");

            ();
        };
        let lang_server_fut = actix::fut::wrap_future(fut);
        ctx.spawn(lang_server_fut);

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
    println!("{:?}", resp);
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
    env_logger::init();
    std::env::set_var("RUST_LOG", "actix_server=info,actix_web=info");

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
                    eprintln!("stdin read failed: {:?}", e);
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
                    eprintln!("pty read failed: {:?}", e);
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
