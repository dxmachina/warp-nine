use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use std::{pin, task};

use futures_lite::{Stream, ready};
use instant::Instant;
use pin::Pin;
use task::{Context, Poll};
use warpui::r#async::Timer;

pub struct Throttle<S> {
    pacing: Pacing,
    stream: S,
    timer: Option<Timer>,
}

/// Decides how long a throttle waits after letting an item through before letting the next
/// one through.
struct Pacing {
    period: Duration,
    /// See [`throttle_quick_after_quiet`].
    quick: Option<(Duration, Arc<AtomicBool>)>,
    last_quick_wait_at: Option<Instant>,
}

impl Pacing {
    /// `was_idle` is whether the item arrived with no wait running, i.e. after the stream had
    /// gone quiet; items that keep coming always arrive during a wait.
    fn wait_after_item(&mut self, now: Instant, was_idle: bool) -> Duration {
        if let Some((quick_period, enabled)) = &self.quick
            && was_idle
            && enabled.load(Ordering::Relaxed)
            && self
                .last_quick_wait_at
                .is_none_or(|last| now.duration_since(last) >= self.period * 2)
        {
            self.last_quick_wait_at = Some(now);
            return *quick_period;
        }
        self.period
    }
}

pub fn throttle<S: Stream>(period: Duration, stream: S) -> impl Stream<Item = S::Item> {
    Throttle {
        pacing: Pacing {
            period,
            quick: None,
            last_quick_wait_at: None,
        },
        stream,
        timer: None,
    }
}

/// Like [`throttle`], except that while `quick` is set, an item that comes after the stream
/// has gone quiet is followed by a wait of only `quick_period`, at most once every two
/// periods. A reply that arrives in two quick parts (an echo, then a redraw) isn't held back
/// by a period, while a stream that keeps producing items is throttled to `period` as usual,
/// however long each item takes to handle.
pub fn throttle_quick_after_quiet<S: Stream>(
    period: Duration,
    quick_period: Duration,
    quick: Arc<AtomicBool>,
    stream: S,
) -> impl Stream<Item = S::Item> {
    Throttle {
        pacing: Pacing {
            period,
            quick: Some((quick_period, quick)),
            last_quick_wait_at: None,
        },
        stream,
        timer: None,
    }
}

impl<S: Stream> Stream for Throttle<S> {
    type Item = S::Item;

    fn poll_next(mut self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if let Some(timer) = self.as_mut().timer() {
            ready!(timer.poll_next(ctx));

            let mut stream = self.as_mut().stream();
            let mut last_poll = Poll::Pending;
            while let Poll::Ready(item) = stream.as_mut().poll_next(ctx) {
                if item.is_none() {
                    // Stream has been closed.
                    self.set_timer(None);
                    return Poll::Ready(None);
                }
                last_poll = Poll::Ready(item)
            }

            if last_poll.is_ready() {
                let wait = self
                    .as_mut()
                    .pacing()
                    .wait_after_item(Instant::now(), false);
                self.set_timer(Some(Timer::after(wait)));
            } else {
                self.set_timer(None);
            }
            last_poll
        } else {
            let item = ready!(self.as_mut().stream().poll_next(ctx));
            let wait = self.as_mut().pacing().wait_after_item(Instant::now(), true);
            self.set_timer(Some(Timer::after(wait)));
            Poll::Ready(item)
        }
    }
}

impl<S> Throttle<S> {
    fn timer(self: Pin<&mut Self>) -> Option<Pin<&mut Timer>> {
        if self.timer.is_some() {
            Some(unsafe { self.map_unchecked_mut(|s| s.timer.as_mut().unwrap()) })
        } else {
            None
        }
    }

    fn stream(self: Pin<&mut Self>) -> Pin<&mut S> {
        unsafe { self.map_unchecked_mut(|s| &mut s.stream) }
    }

    fn pacing(self: Pin<&mut Self>) -> &mut Pacing {
        // SAFETY: `pacing` is never pinned structurally.
        unsafe { &mut self.get_unchecked_mut().pacing }
    }

    fn set_timer(self: Pin<&mut Self>, timer: Option<Timer>) {
        unsafe {
            self.get_unchecked_mut().timer = timer;
        }
    }
}

#[cfg(test)]
#[path = "throttle_tests.rs"]
mod tests;
