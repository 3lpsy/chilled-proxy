//! chilled-proxy binary entry point.

#[tokio::main]
async fn main() {
    chilled_proxy::run().await;
}
