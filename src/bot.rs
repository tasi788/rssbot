use std::borrow::Cow;
use std::future::Future as StdFuture;
use std::time::Duration;

use bytes::Bytes;
use failure::Fail;
use hyper;
use hyper_rustls;
use serde_json;
use telegram_types::bot::{
    methods::{ApiError, GetMe, GetUpdates, Method, TelegramResult, UpdateTypes},
    types::{UpdateContent, UserId},
};
use tokio::{await, prelude::*, timer::timeout};
use tokio_async_await::compat::backward;

use crate::utils::gen_stream;

#[derive(Fail, Debug)]
pub enum Error {
    #[fail(display = "{}", _0)]
    Hyper(#[cause] hyper::Error),
    #[fail(display = "Timeout")]
    Timeout,
    #[fail(display = "{}", _0)]
    Json(#[cause] serde_json::Error),
    #[fail(display = "{}", _0)]
    Telegram(#[cause] ApiError),
    #[fail(display = "{} {:?}", status, body)]
    TelegramServer {
        status: hyper::StatusCode,
        body: Bytes,
    },
}

impl From<hyper::Error> for Error {
    fn from(error: hyper::Error) -> Self {
        Error::Hyper(error)
    }
}

impl From<serde_json::Error> for Error {
    fn from(error: serde_json::Error) -> Self {
        Error::Json(error)
    }
}

impl From<ApiError> for Error {
    fn from(error: ApiError) -> Self {
        Error::Telegram(error)
    }
}

impl<T> From<timeout::Error<T>> for Error
where
    T: Into<Error>,
{
    fn from(error: timeout::Error<T>) -> Self {
        if error.is_inner() {
            error.into_inner().unwrap().into()
        } else if error.is_elapsed() {
            Error::Timeout
        } else if error.is_timer() {
            panic!("Timer error: {:?}", error.into_timer())
        } else {
            unreachable!()
        }
    }
}

pub struct Bot {
    client: hyper::Client<hyper_rustls::HttpsConnector<hyper::client::HttpConnector>, hyper::Body>,
    token: String,
    id: UserId,
    username: String,
    name: String,
    user_agent: String,
}

impl Bot {
    pub async fn new(token: String) -> Result<Bot, Error> {
        let connector = hyper_rustls::HttpsConnector::new(4);
        let client: hyper::Client<_, hyper::Body> = hyper::Client::builder().build(connector);
        let mut this = Bot {
            client: client,
            token: token,
            id: UserId(0),
            username: String::new(),
            name: String::new(),
            user_agent: String::new(),
        };
        let me = await!(this.request(&GetMe, 5))?;
        this.id = me.id;
        this.username = me.username.expect("Bot doesn't have a username");
        this.name = me.first_name;
        this.user_agent = format!(
            concat!(
                env!("CARGO_PKG_NAME"),
                "/",
                env!("CARGO_PKG_VERSION"),
                " (+https://t.me/{})"
            ),
            this.username
        );
        Ok(this)
    }

    fn request<T: serde::Serialize + Method>(
        &self,
        body: &T,
        timeout: u64,
    ) -> impl StdFuture<Output = Result<T::Item, Error>> + 'static {
        let body = serde_json::to_string(body).expect("").into();
        let req = self.client.request(
            hyper::Request::post(T::url(&self.token))
                .header("User-Agent", &*self.user_agent)
                .body(body)
                .expect(""),
        );
        async move {
            let resp = await!(req.timeout(Duration::from_secs(timeout)))?;
            let status = resp.status();
            let body = await!(resp.into_body().concat2())?;
            let r = match serde_json::from_slice::<TelegramResult<T::Item>>(&body) {
                Ok(r) => r,
                Err(_) => {
                    return Err(Error::TelegramServer {
                        status,
                        body: body.into(),
                    })
                }
            };
            let r = <_ as Into<Result<_, _>>>::into(r)?;
            Ok(r)
        }
    }

    fn get_updates<T: Into<Cow<'static, [UpdateTypes]>>>(
        &self,
        timeout: u64,
        filter: T,
    ) -> impl Stream<Item = UpdateContent, Error = Error> + '_ {
        let filter = filter.into();
        let mut offset = None;
        gen_stream(move || loop {
            let body = GetUpdates {
                offset: offset,
                timeout: Some(timeout as i32),
                limit: None,
                allowed_updates: Some(filter.clone()),
            };
            match stream_await!(backward::Compat::new(self.request(&body, timeout))) {
                Ok(resp) => {
                    if let Some(last) = resp.last() {
                        offset = Some(last.update_id);
                    }
                    for update in resp {
                        stream_yield!(update.content);
                    }
                }
                Err(Error::Timeout) => (),
                Err(err) => return Err(err),
            }
        })
    }
}
