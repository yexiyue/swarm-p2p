use std::path::Path;

use anyhow::Result;
use libp2p::identity::Keypair;
use tracing::info;

/// 加载或生成 Ed25519 密钥对
///
/// 密钥以 protobuf 编码保存到文件，与客户端 identity.rs 格式一致。
/// 首次运行自动生成并保存，后续启动从文件加载（PeerId 不变）。
pub fn load_or_generate_keypair(path: &Path) -> Result<Keypair> {
    if path.exists() {
        info!("Loading identity from {:?}", path);
        let bytes = std::fs::read(path)?;
        let keypair = Keypair::from_protobuf_encoding(&bytes)?;
        Ok(keypair)
    } else {
        info!("Generating new Ed25519 identity, saving to {:?}", path);
        let keypair = Keypair::generate_ed25519();
        let bytes = keypair.to_protobuf_encoding()?;
        std::fs::write(path, &bytes)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        }
        Ok(keypair)
    }
}
