use std::env;

fn main() {
    dotenvy::dotenv().ok();

    let base_url = env::var("PUBLIC_URL").unwrap_or_else(|_| "http://localhost:3402".to_string());

    let prompt = env::args().skip(1).collect::<Vec<_>>().join(" ");
    let prompt = if prompt.is_empty() {
        "a cyberpunk cat furiously coding on a laptop with a background like The Matrix virtual world".to_string()
    } else {
        prompt
    };

    println!("Testing image generation...");
    println!("  Server: {}", base_url);
    println!("  Prompt: {}", prompt);
    println!();

    let url = format!(
        "{}/generate_image?prompt={}",
        base_url.trim_end_matches('/'),
        urlencoding::encode(&prompt)
    );

    let resp = reqwest::blocking::get(&url);

    match resp {
        Ok(r) => {
            let status = r.status();
            let body = r.text().unwrap_or_default();

            if status.is_success() {
                let json: serde_json::Value =
                    serde_json::from_str(&body).unwrap_or(serde_json::Value::String(body.clone()));
                println!("OK ({})", status);
                println!("{}", serde_json::to_string_pretty(&json).unwrap_or(body));
            } else if status.as_u16() == 402 {
                println!("GOT 402 — payment required (is TEST_MODE=true set?)");
                println!("{}", body);
            } else {
                println!("ERROR ({})", status);
                println!("{}", body);
            }
        }
        Err(e) => {
            eprintln!("Request failed — is the server running?");
            eprintln!("  {}", e);
            std::process::exit(1);
        }
    }
}
