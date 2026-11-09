use std::sync::Arc;

use axum_test::TestServer;
use cookie::Cookie;
use scorarium_client::fake::FakeTransport;
use scorarium_client::{Client, UserAgent};
use scorarium_engine::auth::Token;
use scorarium_engine::{Engine, EngineOptions};
use scorarium_web::SESSION_COOKIE;
use tempfile::TempDir;

fn engine_options() -> EngineOptions {
    let user_agent = UserAgent::new(Some("Notgnoshi@gmail.com"));
    let transport = FakeTransport::new(&user_agent).expect("failed to open API fixtures");
    let client = Client::with_transport(user_agent.clone(), Arc::new(transport));
    EngineOptions::new(user_agent).unwrap().client(client)
}

async fn open_engine() -> (Engine, TempDir) {
    let dir = TempDir::new().unwrap();
    let engine = Engine::open(dir.path(), engine_options()).await.unwrap();
    (engine, dir)
}

/// Open a demo instance of the [Engine] configured for test [FakeTransport]
pub async fn demo_engine() -> Engine {
    Engine::demo(engine_options()).await.unwrap()
}

/// A test instance of the [Engine] configured with [FakeTransport]
pub struct TestApp {
    pub server: TestServer,
    pub engine: Engine,
    pub token: Option<Token>,
    _dir: Option<TempDir>,
}

impl TestApp {
    /// Get a logged-in demo instance of the test server
    pub async fn demo() -> TestApp {
        let engine = demo_engine().await;
        let mut server = server(&engine);
        let token = engine.login("").await.unwrap();
        server.add_cookie(Cookie::new(SESSION_COOKIE, token.as_str().to_owned()));
        TestApp {
            server,
            engine,
            token: Some(token),
            _dir: None,
        }
    }

    /// Get a logged-out instance of the test server
    pub async fn logged_out(demo: bool) -> TestApp {
        let (engine, dir) = if demo {
            (demo_engine().await, None)
        } else {
            let (engine, dir) = open_engine().await;
            (engine, Some(dir))
        };
        TestApp {
            server: server(&engine),
            engine,
            token: None,
            _dir: dir,
        }
    }
}

fn server(engine: &Engine) -> TestServer {
    TestServer::builder()
        .save_cookies()
        .build(scorarium_web::router(
            engine.clone(),
            scorarium_web::Settings {
                secure_cookies: true,
            },
        ))
}
