# swarm-bootstrap

SwarmDrop 的 DHT 引导 + Relay 中继节点。部署在公网 VPS 上，为客户端提供：

- **Kademlia DHT Server** — 响应路由查询，帮助客户端互相发现
- **Relay Server** — 为 NAT 后的节点中继流量，配合 DCUtR 打洞

## 快速开始

```bash
# 编译
cargo build --release -p swarm-bootstrap

# 运行（首次自动生成 identity.key）
./target/release/swarm-bootstrap
```

启动后输出 `Node PeerId: 12D3KooW...`，记下这个 PeerId，客户端需要用它来配置引导节点地址。

## CLI 参数

```
swarm-bootstrap [OPTIONS]

Options:
    --tcp-port <PORT>       TCP 监听端口          [默认: 4001]
    --quic-port <PORT>      QUIC 监听端口         [默认: 4001]
    --key-file <PATH>       密钥文件路径           [默认: identity.key]
    --listen-addr <IP>      监听 IP 地址           [默认: 0.0.0.0]
    --idle-timeout <SECS>   空闲连接超时(秒)       [默认: 120]
```

日志级别通过 `RUST_LOG` 环境变量控制，默认 `info`。

## 部署

```bash
# 1. 创建用户和目录
sudo useradd -r -s /bin/false -d /opt/swarm-bootstrap swarmdrop
sudo mkdir -p /opt/swarm-bootstrap
sudo chown swarmdrop:swarmdrop /opt/swarm-bootstrap

# 2. 部署二进制
sudo cp target/release/swarm-bootstrap /opt/swarm-bootstrap/

# 3. 安装 systemd 服务
sudo cp swarm-bootstrap.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now swarm-bootstrap

# 4. 查看日志
journalctl -u swarm-bootstrap -f

# 5. 开放防火墙
sudo ufw allow 4001/tcp
sudo ufw allow 4001/udp
```

CI/CD 已配置 GitHub Actions，push 到 `main` 分支且修改了 `bootstrap/` 目录会自动构建部署。

## 密钥管理

- 首次启动自动生成 Ed25519 密钥对，保存为 `identity.key`（protobuf 编码）
- 密钥决定 PeerId，**丢失密钥 = PeerId 改变 = 所有客户端需更新配置**
- `identity.key` 已在 `.gitignore` 中，不会提交到仓库

## 协议栈

| 协议 | 作用 |
|------|------|
| Ping | 心跳保活 |
| Identify | 节点信息交换，`protocol_version` 必须与客户端一致 |
| Kademlia | DHT Server 模式，强制响应所有查询 |
| Relay | 中继服务端，为 NAT 后节点转发流量 |

不包含 mDNS、AutoNAT、DCUtR、Request-Response（引导节点不需要）。

## 设计文档

详见 [docs/bootstrap-relay-node.md](../docs/bootstrap-relay-node.md)。
