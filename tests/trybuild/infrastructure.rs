#[path = "../support/infrastructure.rs"]
mod support;

fn main() {
    let _router = support::webhook_helpers::health_routes();
}
