#![feature(await_macro, async_await, futures_api, generators, generator_trait)]

use futures::prelude::*;
use hyper;
use hyper_rustls;
use tokio::{self, await};

#[macro_use]
mod utils;
mod bot;

fn main() {
    let connector = hyper_rustls::HttpsConnector::new(4);
    let client: hyper::Client<_, hyper::Body> = hyper::Client::builder().build(connector);

    let url = ("https://hyper.rs").parse().unwrap();
    tokio::run_async(
        async move {
            await!(client.get(url));
            println!("Hello");
        },
    );
}
