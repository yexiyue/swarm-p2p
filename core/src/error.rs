use libp2p::noise;
use std::io;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Anyhow(#[from] anyhow::Error),

    #[error("Transport error: {0}")]
    Transport(String),

    #[error("Noise authentication error: {0}")]
    Noise(#[from] noise::Error),

    #[error("IO error: {0}")]
    Io(io::Error),

    #[error("Dial error: {0}")]
    Dial(String),

    #[error("Listen error: {0}")]
    Listen(String),

    #[error("Kad error: {0}")]
    Kad(String),

    #[error("Request-response error: {0}")]
    RequestResponse(String),

    #[error("Behaviour error: {0}")]
    Behaviour(String),
}
