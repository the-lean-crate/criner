use crate::error::{Error, FormatDeadline, Result};
use async_io::Timer;
use dia_semver::Semver;
use futures_util::{
    future::{self, Either},
    FutureExt,
};
use std::{
    convert::TryInto,
    future::Future,
    time::{Duration, SystemTime},
};

pub fn parse_semver(version: &str) -> Semver {
    use std::str::FromStr;
    Semver::from_str(version)
        .or_else(|_| {
            Semver::from_str(
                &version[..version
                    .find('-')
                    .or_else(|| version.find('+'))
                    .expect("some prerelease version")],
            )
        })
        .expect("semver parsing to work if violating prerelease versions are stripped")
}

pub async fn wait_with_progress(
    duration_s: usize,
    progress: prodash::tree::Item,
    deadline: Option<SystemTime>,
    time: Option<time::Time>,
) -> Result<()> {
    progress.init(Some(duration_s), Some("s".into()));
    if let Some(time) = time {
        progress.set_name(format!(
            "{} scheduled at {}",
            progress.name().unwrap_or_else(|| "un-named".into()),
            time.format(&time::macros::format_description!("[hour]:[minute]"))
                .expect("always formattable")
        ));
    }
    for s in 1..=duration_s {
        Timer::after(Duration::from_secs(1)).await;
        check(deadline)?;
        progress.set(s);
    }
    Ok(())
}

fn desired_launch_at(time: Option<time::Time>) -> time::OffsetDateTime {
    let time = time.unwrap_or_else(|| {
        time::OffsetDateTime::now_local()
            .unwrap_or_else(|_| time::OffsetDateTime::now_utc())
            .time()
    });
    let now = time::OffsetDateTime::now_local().unwrap_or_else(|_| time::OffsetDateTime::now_utc());
    let mut desired = now.date().with_time(time).assume_offset(now.offset());
    if desired < now {
        desired = desired
            .date()
            .next_day()
            .expect("not running in year 9999")
            .with_time(time)
            .assume_offset(now.offset());
    }
    desired
}

fn duration_until(time: Option<time::Time>) -> Duration {
    let desired = desired_launch_at(time);
    let now_local = time::OffsetDateTime::now_local().unwrap_or_else(|_| time::OffsetDateTime::now_utc());
    (desired - now_local)
        .try_into()
        .unwrap_or_else(|_| Duration::from_secs(1))
}

pub async fn repeat_daily_at<MakeFut, MakeProgress, Fut, T>(
    time: Option<time::Time>,
    mut make_progress: MakeProgress,
    deadline: Option<SystemTime>,
    mut make_future: MakeFut,
) -> Result<()>
where
    Fut: Future<Output = Result<T>>,
    MakeFut: FnMut() -> Fut,
    MakeProgress: FnMut() -> prodash::tree::Item,
{
    let mut iteration = 0;
    let time = desired_launch_at(time).time();
    loop {
        iteration += 1;
        if let Err(err) = make_future().await {
            make_progress().fail(format!(
                "{} : ignored by repeat_daily_at('{:?}',…) iteration {}",
                err, time, iteration
            ))
        }
        wait_with_progress(
            duration_until(Some(time)).as_secs() as usize,
            make_progress(),
            deadline,
            Some(time),
        )
        .await?;
    }
}

pub async fn repeat_every_s<MakeFut, MakeProgress, Fut, T>(
    interval_s: usize,
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
        wait_with_progress(interval_s, make_progress(), deadline, None).await?;
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

pub async fn handle_ctrl_c_and_sigterm<F, T>(f: F) -> Result<T>
where
    F: Future<Output = T> + Unpin,
{
    let (s, r) = async_channel::bounded(100);
    ctrlc::set_handler(move || {
        s.send(()).now_or_never();
    })
    .ok();
    let selector = future::select(async move { r.recv().await }.boxed_local(), f);
    match selector.await {
        Either::Left((_, _f)) => Err(Error::Interrupted),
        Either::Right((r, _interrupt)) => Ok(r),
    }
}

pub async fn timeout_after<F, T>(duration: Duration, msg: impl Into<String>, f: F) -> Result<T>
where
    F: Future<Output = T> + Unpin,
{
    let selector = future::select(Timer::after(duration), f);
    match selector.await {
        Either::Left((_, _f)) => Err(Error::Timeout(duration, msg.into())),
        Either::Right((r, _delay)) => Ok(r),
    }
}

/// Use this if `f()` might block forever, due to code that doesn't implement timeouts like libgit2 fetch does as it has no timeout
/// on 'recv' bytes.
///
/// This approach eventually fails as we would accumulate more and more threads, but this will also give use additional
/// days of runtime for little effort. On a Chinese network, outside of data centers, one can probably restart criner on
/// a weekly basis or so, which is can easily be automated.
pub async fn enforce_threaded<F, T>(deadline: SystemTime, f: F) -> Result<T>
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    let unblocked = blocking::unblock(f);
    let selector = future::select(
        Timer::after(deadline.duration_since(SystemTime::now()).unwrap_or_default()),
        unblocked.boxed(),
    );
    match selector.await {
        Either::Left((_, _f_as_future)) => Err(Error::DeadlineExceeded(FormatDeadline(deadline))),
        Either::Right((res, _delay)) => Ok(res),
    }
}
