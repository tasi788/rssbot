#[macro_export]
macro_rules! stream_yield {
    ($e:expr) => {{
        let e = $e;
        yield ::futures::Async::Ready(e)
    }};
}

#[macro_export]
macro_rules! stream_await {
    ($e:expr) => ({
        let mut future = $e;
        loop {
            match ::futures::Future::poll(&mut future) {
                ::std::result::Result::Ok(::futures::Async::Ready(e)) => {
                    break ::std::result::Result::Ok(e)
                }
                ::std::result::Result::Ok(::futures::Async::NotReady) => {}
                ::std::result::Result::Err(e) => {
                    break ::std::result::Result::Err(e)
                }
            }
            yield ::futures::Async::NotReady
        }
    })
}

pub use std::ops::Generator;

use futures::Poll;
use futures::{Async, Stream};
use std::marker::PhantomData;
use std::ops::GeneratorState;

pub trait MyStream<T, U: IsResult<Ok = ()>>: Stream<Item = T, Error = U::Err> {}

impl<F, T, U> MyStream<T, U> for F
where
    F: Stream<Item = T, Error = U::Err> + ?Sized,
    U: IsResult<Ok = ()>,
{}

pub trait IsResult {
    type Ok;
    type Err;

    fn into_result(self) -> Result<Self::Ok, Self::Err>;
}

impl<T, E> IsResult for Result<T, E> {
    type Ok = T;
    type Err = E;

    fn into_result(self) -> Result<Self::Ok, Self::Err> {
        self
    }
}

/// Small shim to translate from a generator to a stream.
struct GenStream<U, T> {
    gen: T,
    done: bool,
    phantom: PhantomData<U>,
}

pub fn gen_stream<T, U>(gen: T) -> impl MyStream<U, T::Return>
where
    T: Generator<Yield = Async<U>>,
    T::Return: IsResult<Ok = ()>,
{
    GenStream {
        gen,
        done: false,
        phantom: PhantomData,
    }
}

impl<U, T> Stream for GenStream<U, T>
where
    T: Generator<Yield = Async<U>>,
    T::Return: IsResult<Ok = ()>,
{
    type Item = U;
    type Error = <T::Return as IsResult>::Err;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        if self.done {
            return Ok(Async::Ready(None));
        }
        match unsafe { self.gen.resume() } {
            GeneratorState::Yielded(Async::Ready(e)) => Ok(Async::Ready(Some(e))),
            GeneratorState::Yielded(Async::NotReady) => Ok(Async::NotReady),
            GeneratorState::Complete(e) => {
                self.done = true;
                e.into_result().map(|()| Async::Ready(None))
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn generator_stream() {
        let stream = gen_stream(|| {
            stream_yield!(false);
            stream_yield!(true);
            Ok::<(), ()>(())
        });
        let r = stream.wait().collect::<Result<Vec<_>, _>>();
        assert_eq!(r, Ok(vec![false, true]))
    }
}
