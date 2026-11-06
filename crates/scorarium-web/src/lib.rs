use std::net::SocketAddr;

#[derive(Debug, clap::Args)]
pub struct ServeArgs {
    /// Address and port to serve on.
    #[arg(short, long, env = "SCORARIUM_BIND", default_value = "0.0.0.0:3000")]
    pub bind: SocketAddr,

    /// Allow the login cookie over plain HTTP
    #[arg(long, env = "SCORARIUM_INSECURE_COOKIES")]
    pub insecure_cookies: bool,
}
