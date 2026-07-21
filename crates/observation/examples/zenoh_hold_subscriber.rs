//! Hold a peer Zenoh subscriber open so Gateway `--zenoh` can pass its install gate.
//!
//! Usage:
//! cargo run -p observation --example zenoh_hold_subscriber -- \
//! --keyexpr sdv/twin/observation --hold-secs 15

use std::env;
use std::process;
use std::time::Duration;

use observation::ZenohLiveSource;

fn usage() -> ! {
    eprintln!(
        "usage: zenoh_hold_subscriber --keyexpr <expr> [--hold-secs <n>]\n\
         \n\
         Holds a peer Zenoh subscriber so Gateway can wait for matching and install."
    );
    process::exit(2);
}

fn main() {
    let mut keyexpr: Option<String> = None;
    let mut hold_secs: u64 = 15;
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--keyexpr" => {
                keyexpr = Some(args.next().unwrap_or_else(|| usage()));
            }
            "--hold-secs" => {
                let raw = args.next().unwrap_or_else(|| usage());
                hold_secs = raw.parse().unwrap_or_else(|_| usage());
            }
            "-h" | "--help" => usage(),
            _ => usage(),
        }
    }
    let Some(keyexpr) = keyexpr.filter(|k| !k.is_empty()) else {
        usage();
    };

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("runtime");
    rt.block_on(async move {
        let mut source = ZenohLiveSource::subscribe(keyexpr.clone())
            .await
            .unwrap_or_else(|err| {
                eprintln!("subscribe failed: {err}");
                process::exit(1);
            });
        eprintln!("[zenoh_hold_subscriber] subscribed on {keyexpr}; holding {hold_secs}s");
        let deadline = tokio::time::Instant::now() + Duration::from_secs(hold_secs);
        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                break;
            }
            match tokio::time::timeout(remaining, source.recv()).await {
                Ok(Ok(Some(_))) => {}
                Ok(Ok(None)) | Ok(Err(_)) => break,
                Err(_) => break,
            }
        }
        eprintln!("[zenoh_hold_subscriber] done");
    });
}
