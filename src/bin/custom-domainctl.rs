use axum::{routing::post, Router};
use custom_domain_release_hook::{
    domain_release::ReleaseLedger,
    infrai_client::{InfraiClient, RegisterWebhook},
    webhook_receiver::{receive, WebhookState},
};
use serde_json::json;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("onboard") => onboard(&args).await?,
        Some("serve") => serve(&args).await?,
        _ => {
            eprintln!("usage: custom-domainctl onboard <domain> <target> <build-id> <webhook-url> | serve <build-id> <domain>");
            std::process::exit(2);
        }
    }
    Ok(())
}

async fn onboard(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    if args.len() != 6 {
        return Err("onboard needs <domain> <target> <build-id> <webhook-url>".into());
    }
    let domain = &args[2];
    let target = &args[3];
    let build_id = &args[4];
    let webhook_url = &args[5];
    let secret = std::env::var("INFRAI_WEBHOOK_SECRET")?;
    let client = InfraiClient::from_env()?;

    let added = client.add_domain(domain, build_id).await?;
    client
        .upsert_cname(&added.zone_id, domain, target, build_id)
        .await?;
    client.verify_domain(domain).await?;
    client
        .register_webhook(RegisterWebhook {
            url: webhook_url,
            events: vec!["dns.domain.verified"],
            description: "Release a build after custom-domain verification",
            secret: &secret,
        })
        .await?;

    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "build_id": build_id,
            "domain": added.domain,
            "zone_id": added.zone_id,
            "state": "waiting_for_domain",
            "diagnostic": "verification requested; release continues from the signed webhook"
        }))?
    );
    Ok(())
}

async fn serve(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    if args.len() != 4 {
        return Err("serve needs <build-id> <domain>".into());
    }
    let secret = std::env::var("INFRAI_WEBHOOK_SECRET")?;
    let bind = std::env::var("BIND_ADDR").unwrap_or_else(|_| "127.0.0.1:3000".into());
    let ledger = ReleaseLedger::default();
    let pending = ledger.track(args[2].clone(), args[3].clone()).await;
    let app = Router::new()
        .route("/webhooks/infrai", post(receive))
        .with_state(WebhookState { secret, ledger });
    let listener = tokio::net::TcpListener::bind(&bind).await?;
    eprintln!(
        "listening on {bind}; {} is {:?}",
        pending.build_id, pending.state
    );
    axum::serve(listener, app).await?;
    Ok(())
}
