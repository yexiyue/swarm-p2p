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

    #[error("Kad store error: {0}")]
    KadStore(#[from] libp2p::kad::store::Error),

    #[error("Kad provide error: {0}")]
    KadProvide(String),

    #[error("Kad get providers error: {0}")]
    KadGetProviders(String),

    #[error("Kad get record error: {0}")]
    KadGetRecord(String),

    #[error("Kad put record error: {0}")]
    KadPutRecord(String),

    #[error("Kad get closest peers error: {0}")]
    KadGetClosestPeers(String),

    #[error("Listen error: {0}")]
    Listen(String),

    #[error("Behaviour error: {0}")]
    Behaviour(String),

    #[error("Config error: {0}")]
    Config(String),
}
