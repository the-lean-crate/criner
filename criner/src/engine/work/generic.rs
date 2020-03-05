use crate::{
    error::{Error, Result},
    model, persistence,
    persistence::TreeAccess,
};
use async_trait::async_trait;

#[async_trait]
pub trait Processor {
    type Item;

    fn set(&mut self, request: Self::Item, out_key: &mut String) -> Result<(model::Task, String)>;
    fn idle_message(&self) -> String;
    async fn process(&mut self) -> std::result::Result<(), (Error, String)>;
}

pub async fn processor<T>(
    db: persistence::Db,
    mut progress: prodash::tree::Item,
    r: async_std::sync::Receiver<T>,
    mut agent: impl Processor<Item = T>,
) -> Result<()> {
    let mut key = String::with_capacity(32);
    let tasks = db.open_tasks()?;

    while let Some(request) = r.recv().await {
        let (dummy_task, progress_info) = agent.set(request, &mut key)?;
        progress.set_name(progress_info);
        progress.init(None, None);

        let mut task = tasks.update(&key, |mut t| {
            t.process = dummy_task.process.clone();
            t.version = dummy_task.version.clone();
            t.state.merge_with(&model::TaskState::InProgress(None));
            t
        })?;

        progress.blocked(None);
        let res = agent.process().await;

        task.state = match res {
            Ok(_) => model::TaskState::Complete,
            Err((err, msg)) => {
                progress.fail(format!("{}: {}", msg, err));
                model::TaskState::AttemptsWithFailure(vec![err.to_string()])
            }
        };

        tasks.upsert(&key, &task)?;
        progress.set_name(agent.idle_message());
        progress.init(None, None);
    }
    Ok(())
}
