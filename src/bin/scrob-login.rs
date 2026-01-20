use std::io::{self, Write};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize)]
struct LoginRequest {
    username: String,
    password: String,
}

#[derive(Debug, Deserialize)]
struct LoginResponse {
    token: String,
    username: String,
    is_admin: bool,
}

#[derive(Debug, Deserialize)]
struct ErrorResponse {
    error: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Scrob Login ===");
    println!();

    // Get server URL
    let server_url = if let Ok(url) = std::env::var("SCROB_SERVER_URL") {
        println!("Using server: {}", url);
        url
    } else {
        print!("Enter scrob server URL (e.g., https://scrob.yourdomain.com): ");
        io::stdout().flush()?;
        let mut url = String::new();
        io::stdin().read_line(&mut url)?;
        url.trim().to_string()
    };

    // Remove /graphql suffix if present (login is REST, not GraphQL)
    let base_url = server_url
        .trim_end_matches('/')
        .trim_end_matches("/graphql");

    // Get username
    print!("Username: ");
    io::stdout().flush()?;
    let mut username = String::new();
    io::stdin().read_line(&mut username)?;
    let username = username.trim().to_string();

    // Get password (note: this will be visible in terminal)
    print!("Password: ");
    io::stdout().flush()?;
    let mut password = String::new();
    io::stdin().read_line(&mut password)?;
    let password = password.trim().to_string();

    println!();
    println!("Logging in...");

    // Make login request
    let client = reqwest::Client::new();
    let login_url = format!("{}/login", base_url);

    let response = client
        .post(&login_url)
        .json(&LoginRequest { username, password })
        .send()
        .await?;

    if response.status().is_success() {
        let login_response: LoginResponse = response.json().await?;

        println!();
        println!("✓ Login successful!");
        println!();
        println!("Username: {}", login_response.username);
        println!("Admin: {}", if login_response.is_admin { "yes" } else { "no" });
        println!();
        println!("Your token:");
        println!("{}", login_response.token);
        println!();
        println!("Add these to your shell environment:");
        println!();
        println!("export SCROB_SERVER_URL=\"{}/graphql\"", base_url);
        println!("export SCROB_TOKEN=\"{}\"", login_response.token);
        println!();
        println!("Or add to your ~/.bashrc, ~/.zshrc, or Nushell env.nu:");
        println!();
        println!("# Bash/Zsh:");
        println!("echo 'export SCROB_SERVER_URL=\"{}/graphql\"' >> ~/.bashrc", base_url);
        println!("echo 'export SCROB_TOKEN=\"{}\"' >> ~/.bashrc", login_response.token);
        println!();
        println!("# Nushell:");
        println!("echo '$env.SCROB_SERVER_URL = \"{}/graphql\"' >> ~/.config/nushell/env.nu", base_url);
        println!("echo '$env.SCROB_TOKEN = \"{}\"' >> ~/.config/nushell/env.nu", login_response.token);
        println!();
    } else {
        let status = response.status();
        let error_text = response.text().await?;

        // Try to parse as JSON error
        if let Ok(error_response) = serde_json::from_str::<ErrorResponse>(&error_text) {
            eprintln!("✗ Login failed: {}", error_response.error);
        } else {
            eprintln!("✗ Login failed ({}): {}", status, error_text);
        }

        std::process::exit(1);
    }

    Ok(())
}
