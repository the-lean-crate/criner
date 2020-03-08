use crate::{
    error::{Error, Result},
    model, persistence,
    persistence::TableAccess,
};
use async_trait::async_trait;

#[async_trait]
pub trait Processor {
    type Item;

    fn set(
        &mut self,
        request: Self::Item,
        out_key: &mut String,
        progress: &mut prodash::tree::Item,
    ) -> Result<(model::Task, String)>;
    fn idle_message(&self) -> String;
    async fn process(
        &mut self,
        progress: &mut prodash::tree::Item,
    ) -> std::result::Result<(), (Error, String)>;
    async fn schedule_next(
        &mut self,
        _progress: &mut prodash::tree::Item,
    ) -> std::result::Result<(), Error> {
        Ok(())
    }
}

pub async fn processor<T>(
    db: persistence::Db,
    mut progress: prodash::tree::Item,
    r: async_std::sync::Receiver<T>,
    mut agent: impl Processor<Item = T> + Send,
) -> Result<()> {
    let mut key = String::with_capacity(32);
    let tasks = db.open_tasks()?;

    while let Some(request) = r.recv().await {
        key.clear();
        let (dummy_task, progress_info) = agent.set(request, &mut key, &mut progress)?;
        progress.set_name(progress_info);

        let mut task = tasks.update(Some(&mut progress), &key, |mut t| {
            t.process = dummy_task.process.clone();
            t.version = dummy_task.version.clone();
            t.state.merge_with(&model::TaskState::InProgress(None));
            t
        })?;

        progress.blocked("working", None);
        let res = agent.process(&mut progress).await;

        task.state = match res {
            Ok(_) => {
                agent.schedule_next(&mut progress).await.ok();
                model::TaskState::Complete
            }
            Err((err, msg)) => {
                progress.fail(format!("{}: {}", msg, err));
                model::TaskState::AttemptsWithFailure(vec![err.to_string()])
            }
        };

        tasks.upsert(&mut progress, &key, &task)?;
        progress.set_name(agent.idle_message());
        progress.init(None, None);
    }
    Ok(())
}
