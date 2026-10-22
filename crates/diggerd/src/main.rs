//! diggerd 薄入口:参数解析 + 调用 dozerd::daemon::run。
//! 产品身份由 digger_paths::product() 提供,功能开关用 DaemonFeatures::minimal()。

use anyhow::Result;
use dozer_core::paths::ProductId;
use dozerd::daemon::{DaemonConfig, DaemonFeatures};
use std::path::PathBuf;

#[tokio::main]
async fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let mut socket: Option<PathBuf> = None;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--socket" => match args.next() {
                Some(p) => socket = Some(PathBuf::from(p)),
                None => {
                    eprintln!("--socket 需要一个路径参数");
                    std::process::exit(2);
                }
            },
            "--version" => {
                println!("diggerd {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            other => {
                eprintln!("未知参数: {other}(支持 --socket <path> / --version)");
                std::process::exit(2);
            }
        }
    }
    let product: ProductId = digger_paths::product();
    let mut config = DaemonConfig {
        product,
        socket: digger_paths::product().socket_path(),
        features: DaemonFeatures::minimal(),
    };
    if let Some(socket) = socket {
        config.socket = socket;
    }
    dozerd::daemon::run(config).await
}
