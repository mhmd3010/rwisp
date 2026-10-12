use std::env;
use std::io::{self, Write};
use std::net::IpAddr;
use std::process::ExitCode;
use std::time::Duration;

use rwisp_server::crypto::EncryptionKey;
use rwisp_server::db::init_pool;
use rwisp_server::routers::{self, NewRouter, RouterRecord};
use rwisp_server::rwispros::Transport;

#[tokio::main]
async fn main() -> ExitCode {
    dotenvy::dotenv().ok();

    let args: Vec<String> = env::args().skip(1).collect();
    match run(args).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

async fn run(args: Vec<String>) -> Result<(), String> {
    match args.first().map(String::as_str) {
        Some("add") => add_router().await,
        Some("list") => list_command().await,
        Some("show") => show_router(parse_id_arg(&args)?).await,
        Some("help" | "-h" | "--help") | None => {
            print_usage();
            Ok(())
        }
        Some(other) => {
            print_usage();
            Err(format!("unknown command: {other}"))
        }
    }
}

fn parse_id_arg(args: &[String]) -> Result<u64, String> {
    let raw = args
        .get(1)
        .ok_or_else(|| "show requires a router id: rwisp-router show <id>".to_string())?;
    raw.trim()
        .parse::<u64>()
        .map_err(|_| format!("invalid router id: {raw}"))
}

async fn add_router() -> Result<(), String> {
    println!("Register a new MikroTik router");
    println!();

    let host = prompt_required("Host")?;
    let transport = prompt_transport()?;
    let port = prompt_port(transport.default_port())?;
    let username = prompt_required("API username")?;
    let timeout = prompt_timeout(10)?;
    let radius_nas_ip = prompt_optional_ip("RADIUS/NAS IP")?;
    let name = prompt_with_default("Name", &host)?;
    let password = prompt_password_confirmed()?;

    let key = EncryptionKey::from_env().map_err(|error| error.to_string())?;
    let pool = init_pool()
        .await
        .map_err(|error| format!("database connection failed: {error}"))?;

    let mut router = NewRouter::new(&name, &host, &username, password)
        .with_port(port)
        .with_transport(transport)
        .with_timeout(timeout);
    if let Some(ip) = radius_nas_ip {
        router = router.with_radius_nas_ip(ip);
    }

    let id = routers::create_router(&pool, &router, &key)
        .await
        .map_err(|error| error.to_string())?;

    println!();
    println!("Registered router {id} ({name})");
    Ok(())
}

async fn list_command() -> Result<(), String> {
    let pool = init_pool()
        .await
        .map_err(|error| format!("database connection failed: {error}"))?;
    let records = routers::list_routers(&pool, true)
        .await
        .map_err(|error| error.to_string())?;

    if records.is_empty() {
        println!("No routers registered.");
        return Ok(());
    }

    print_router_table(&records);
    Ok(())
}

async fn show_router(id: u64) -> Result<(), String> {
    let pool = init_pool()
        .await
        .map_err(|error| format!("database connection failed: {error}"))?;
    let record = routers::get_router(&pool, id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("router {id} was not found"))?;

    print_router_details(&record);
    Ok(())
}

fn print_router_table(records: &[RouterRecord]) {
    println!(
        "{:<4} {:<24} {:<24} {:<6} {:<13} {:<8} {:<16} {:<7}",
        "ID", "NAME", "HOST", "PORT", "TRANSPORT", "TIMEOUT", "RADIUS_NAS_IP", "ENABLED"
    );
    for record in records {
        let timeout = format!("{}s", record.timeout.as_secs());
        let nas_ip = record.radius_nas_ip.as_deref().unwrap_or("-");
        let enabled = if record.enabled { "yes" } else { "no" };
        println!(
            "{:<4} {:<24} {:<24} {:<6} {:<13} {:<8} {:<16} {:<7}",
            record.id,
            record.name,
            record.host,
            record.api_port,
            record.transport.as_str(),
            timeout,
            nas_ip,
            enabled
        );
    }
}

fn print_router_details(record: &RouterRecord) {
    println!("ID:            {}", record.id);
    println!("Name:          {}", record.name);
    println!("Host:          {}", record.host);
    println!("API port:      {}", record.api_port);
    println!("Username:      {}", record.username);
    println!("Transport:     {}", record.transport.as_str());
    println!("Timeout:       {}s", record.timeout.as_secs());
    println!(
        "RADIUS/NAS IP: {}",
        record.radius_nas_ip.as_deref().unwrap_or("-")
    );
    println!(
        "Enabled:       {}",
        if record.enabled { "yes" } else { "no" }
    );
    println!("Created at:    {}", record.created_at);
    println!("Updated at:    {}", record.updated_at);
}

fn print_usage() {
    println!("rwisp-router - manage MikroTik routers registered in MariaDB");
    println!();
    println!("USAGE:");
    println!("    rwisp-router add          Register a router interactively");
    println!("    rwisp-router list         List registered routers");
    println!("    rwisp-router show <id>    Show a registered router");
    println!("    rwisp-router help         Show this message");
}

fn prompt(label: &str) -> Result<String, String> {
    print!("{label}: ");
    io::stdout().flush().map_err(|error| error.to_string())?;

    let mut line = String::new();
    io::stdin()
        .read_line(&mut line)
        .map_err(|error| format!("failed to read input: {error}"))?;

    Ok(line.trim().to_string())
}

fn prompt_required(label: &str) -> Result<String, String> {
    loop {
        let value = prompt(label)?;
        if !value.is_empty() {
            return Ok(value);
        }
        eprintln!("{label} is required");
    }
}

fn prompt_with_default(label: &str, default: &str) -> Result<String, String> {
    let value = prompt(&format!("{label} [{default}]"))?;
    if value.is_empty() {
        Ok(default.to_string())
    } else {
        Ok(value)
    }
}

fn prompt_transport() -> Result<Transport, String> {
    loop {
        let value = prompt_with_default("Transport (tcp/tls/tls_insecure)", "tcp")?;
        match Transport::parse(&value) {
            Some(transport) => return Ok(transport),
            None => eprintln!("transport must be one of: tcp, tls, tls_insecure"),
        }
    }
}

fn prompt_port(default: u16) -> Result<u16, String> {
    loop {
        let value = prompt_with_default("API port", &default.to_string())?;
        match value.parse::<u16>() {
            Ok(port) if port > 0 => return Ok(port),
            _ => eprintln!("port must be a number between 1 and 65535"),
        }
    }
}

fn prompt_timeout(default: u64) -> Result<Duration, String> {
    loop {
        let value = prompt_with_default("Connection timeout (seconds)", &default.to_string())?;
        match value.parse::<u64>() {
            Ok(seconds) if seconds > 0 => return Ok(Duration::from_secs(seconds)),
            _ => eprintln!("timeout must be a positive number of seconds"),
        }
    }
}

fn prompt_optional_ip(label: &str) -> Result<Option<String>, String> {
    loop {
        let value = prompt(&format!("{label} (optional)"))?;
        if value.is_empty() {
            return Ok(None);
        }
        match value.parse::<IpAddr>() {
            Ok(ip) => return Ok(Some(ip.to_string())),
            Err(_) => eprintln!("{label} must be a valid IP address, or blank to skip"),
        }
    }
}

fn prompt_password_confirmed() -> Result<String, String> {
    loop {
        let first = rpassword::prompt_password("API password: ")
            .map_err(|error| format!("failed to read password: {error}"))?;
        if first.is_empty() {
            eprintln!("password is required");
            continue;
        }

        let second = rpassword::prompt_password("Confirm password: ")
            .map_err(|error| format!("failed to read password: {error}"))?;
        if first == second {
            return Ok(first);
        }
        eprintln!("passwords do not match, please try again");
    }
}
