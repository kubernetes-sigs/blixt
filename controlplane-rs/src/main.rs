/*
Copyright 2024 The Kubernetes Authors.

Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

    http://www.apache.org/licenses/LICENSE-2.0

Unless required by applicable law or agreed to in writing, software
distributed under the License is distributed on an "AS IS" BASIS,
WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
See the License for the specific language governing permissions and
limitations under the License.
*/

use controlplane::*;
use kube::Client;
use tracing::*;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    run().await;
    Ok(())
}

pub async fn run() {
    let subscriber = tracing_subscriber::FmtSubscriber::new();
    tracing::subscriber::set_global_default(subscriber).unwrap();

    let client = Client::try_default()
        .await
        .expect("failed to create kube Client");
    let ctx = Context {
        client: client.clone(),
    };

    if let Err(error) = gateway_controller::controller(ctx).await {
        error!("failed to start Gateway contoller: {error:?}");
        std::process::exit(1);
    }
}
