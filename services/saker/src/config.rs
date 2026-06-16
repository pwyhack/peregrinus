use std::{env, net::SocketAddr};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    pub bind_addr: SocketAddr,
}

impl Settings {
    pub fn from_env() -> anyhow::Result<Self> {
        let host = env::var("SAKER_HOST").unwrap_or_else(|_| "127.0.0.1".into());
        let port = env::var("SAKER_PORT").unwrap_or_else(|_| "1314".into());
        let bind_addr = format!("{host}:{port}").parse()?;

        Ok(Self { bind_addr })
    }
}
