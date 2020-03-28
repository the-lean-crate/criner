use crate::{model, persistence, persistence::TableAccess, Error, Result};
use async_trait::async_trait;

#[async_trait]
pub trait Processor {
    type Item;

    fn set(
        &mut self,
        request: Self::Item,
        progress: &mut prodash::tree::Item,
    ) -> Result<(model::Task, String, String)>;
    fn idle_message(&self) -> String;
    async fn process(
        &mut self,
        progress: &mut prodash::tree::Item,
    ) -> std::result::Result<(), (Error, String)>;
    async fn schedule_next(&mut self, _progress: &mut prodash::tree::Item) -> Result<()> {
        Ok(())
    }
}

pub async fn processor<T: Clone>(
    db: persistence::Db,
    mut progress: prodash::tree::Item,
    r: async_std::sync::Receiver<T>,
    mut agent: impl Processor<Item = T> + Send,
    max_retries_on_timeout: usize,
) -> Result<()> {
    let tasks = db.open_tasks()?;

    while let Some(request) = r.recv().await {
        let mut try_count = 0;
        let (task, task_key) = loop {
            let (dummy_task, task_key, progress_name) =
                agent.set(request.clone(), &mut progress)?;
            progress.set_name(progress_name);

            let mut task = tasks.update(Some(&mut progress), &task_key, |mut t| {
                t.process = dummy_task.process.clone();
                t.version = dummy_task.version.clone();
                t.state.merge_with(&model::TaskState::InProgress(None));
                t
            })?;

            try_count += 1;
            progress.blocked("working", None);
            let res = agent.process(&mut progress).await;

            task.state = match res {
                Err((err @ Error::Timeout(_, _), _)) if try_count < max_retries_on_timeout => {
                    progress.fail(format!(
                        "{} → retrying ({}/{})",
                        err, try_count, max_retries_on_timeout
                    ));
                    continue;
                }
                Err((err, msg)) => {
                    progress.fail(format!("{}: {}", msg, err));
                    model::TaskState::AttemptsWithFailure(vec![err.to_string()])
                }
                Ok(_) => {
                    agent.schedule_next(&mut progress).await.ok();
                    model::TaskState::Complete
                }
            };
            break (task, task_key);
        };

        tasks.upsert(&mut progress, &task_key, &task)?;
        progress.set_name(agent.idle_message());
        progress.init(None, None);
    }
    Ok(())
}
