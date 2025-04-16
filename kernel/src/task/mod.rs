
use core::{future::Future, pin::Pin, task::{Context, Poll}};
use alloc::boxed::Box;

pub mod simple_executor;
pub mod keyboard;

pub struct Task {
    future: Pin<Box<dyn Future<Output = ()>>>,
}

impl Task {
    pub fn new<F>(future: F) -> Self
    where
        F: Future<Output = ()> + 'static,
    {
        Self { future: Box::pin(future) }
    }

    fn poll(&mut self, context: &mut Context) -> Poll<()> {
        self.future.as_mut().poll(context)
    }
}