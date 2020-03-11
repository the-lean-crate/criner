use crate::error::{Error, FormatDeadline, Result};
use futures::task::SpawnExt;
use futures::{
    future::{self, Either},
    task::Spawn,
};
use futures_timer::Delay;
use std::{future::Future, time::Duration, time::SystemTime};

pub async fn wait_with_progress(
    duration_s: u32,
    mut progress: prodash::tree::Item,
    deadline: Option<SystemTime>,
) -> Result<()> {
    progress.init(Some(duration_s), Some("s"));
    for s in 1..=duration_s {
        Delay::new(Duration::from_secs(1)).await;
        check(deadline)?;
        progress.set(s);
    }
    Ok(())
}

pub async fn repeat_every_s<MakeFut, MakeProgress, Fut, T>(
    interval_s: u32,
    mut make_progress: MakeProgress,
    deadline: Option<SystemTime>,
    at_most: Option<usize>,
    mut make_future: MakeFut,
) -> Result<()>
where
    Fut: Future<Output = Result<T>>,
    MakeFut: FnMut() -> Fut,
    MakeProgress: FnMut() -> prodash::tree::Item,
{
    let max_iterations = at_most.unwrap_or(std::usize::MAX);
    let mut iteration = 0;
    loop {
        if iteration == max_iterations {
            return Ok(());
        }
        iteration += 1;
        if let Err(err) = make_future().await {
            make_progress().fail(format!(
                "{} : ignored by repeat_every({}s,…) iteration {}",
                err, interval_s, iteration
            ))
        }
        if iteration == max_iterations {
            return Ok(());
        }
        wait_with_progress(interval_s, make_progress(), deadline).await?;
    }
}

pub fn check(deadline: Option<SystemTime>) -> Result<()> {
    deadline
        .map(|d| {
            if SystemTime::now() >= d {
                Err(Error::DeadlineExceeded(FormatDeadline(d)))
            } else {
                Ok(())
            }
        })
        .unwrap_or(Ok(()))
}

pub async fn enforce<F, T>(deadline: Option<SystemTime>, f: F) -> Result<T>
where
    F: Future<Output = T> + Unpin,
{
    match deadline {
        Some(d) => {
            let selector = future::select(
                Delay::new(d.duration_since(SystemTime::now()).unwrap_or_default()),
                f,
            );
            match selector.await {
                Either::Left((_, _f)) => Err(Error::DeadlineExceeded(FormatDeadline(d))),
                Either::Right((r, _delay)) => Ok(r),
            }
        }
        None => Ok(f.await),
    }
}

pub async fn enforce_blocking<F, T>(deadline: Option<SystemTime>, f: F, s: impl Spawn) -> Result<T>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    enforce(deadline, s.spawn_with_handle(async { f() })?).await
}
