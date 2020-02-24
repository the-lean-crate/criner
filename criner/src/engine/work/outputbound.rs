use crate::error::Result;

pub async fn processor<T>(
    mut progress: prodash::tree::Item,
    input: async_std::sync::Receiver<
        impl FnOnce(prodash::tree::Item) -> futures::future::BoxFuture<'static, T>,
    >,
    output: async_std::sync::Sender<T>,
) -> Result<()> {
    while let Some(make_fut) = input.recv().await {
        output.send(make_fut(progress.add_child("")).await).await;
        progress.set_name("🏋 → 🔆")
    }
    Ok(())
}
