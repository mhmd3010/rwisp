use std::process::ExitCode;

use rwisp_server::db::init_pool;
use rwisp_server::radius::run_maintenance;

#[tokio::main]
async fn main() -> ExitCode {
    dotenvy::dotenv().ok();

    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("maintenance") | None => match maintenance().await {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("error: {error}");
                ExitCode::FAILURE
            }
        },
        Some(other) => {
            eprintln!("unknown command: {other}");
            usage();
            ExitCode::FAILURE
        }
    }
}

async fn maintenance() -> Result<(), String> {
    let pool = init_pool().await.map_err(|error| error.to_string())?;
    let report = run_maintenance(&pool)
        .await
        .map_err(|error| error.to_string())?;

    println!("subscriptions_expired={}", report.subscriptions_expired);
    println!("vouchers_expired={}", report.vouchers_expired);
    println!("renewals_activated={}", report.renewals_activated);
    println!("disconnects={}", report.disconnects.len());
    for target in &report.disconnects {
        println!(
            "disconnect session_id={} username={} nas_ip={} acct_session_id={}",
            target.session_id, target.username, target.nas_ip, target.acct_session_id
        );
    }
    Ok(())
}

fn usage() {
    println!("rwisp-radius - RADIUS lifecycle maintenance");
    println!();
    println!("USAGE:");
    println!(
        "    rwisp-radius maintenance    Expire overdue plans, promote due renewals, list disconnects"
    );
}
