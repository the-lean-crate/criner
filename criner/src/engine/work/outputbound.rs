use crate::error::Result;

pub async fn processor<T>(
    mut progress: prodash::tree::Item,
    input: async_std::sync::Receiver<futures::future::BoxFuture<'_, T>>,
    output: async_std::sync::Sender<T>,
) -> Result<()> {
    while let Some(fut) = input.recv().await {
        output.send(fut.await).await;
        progress.set_name("🏋 → 🔆")
    }
    Ok(())
}
