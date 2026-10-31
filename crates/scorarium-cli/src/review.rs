use std::io::Write;
use std::time::SystemTime;

use scorarium_engine::Engine;
use scorarium_engine::entity::Timestamp;

use crate::library::write_table;
use crate::search::describe;

/// List the review queue
pub(crate) async fn run(engine: &Engine, out: &mut impl Write) -> eyre::Result<()> {
    let now = Timestamp::from(SystemTime::now());
    let rows: Vec<[String; 5]> = engine
        .review_queue()
        .await?
        .into_iter()
        .map(|item| {
            let (kind, id, label) = describe(&item.entity);
            let seconds = now.signed_duration_since(item.queued_at).num_seconds();
            [item.library_name, kind.to_string(), id, label, age(seconds)]
        })
        .collect();
    write_table(out, &rows)
}

fn age(seconds: i64) -> String {
    match seconds {
        ..=0 => "0s".to_string(),
        1..60 => format!("{seconds}s"),
        60..3600 => format!("{}m", seconds / 60),
        3600..86400 => format!("{}h", seconds / 3600),
        _ => format!("{}d", seconds / 86400),
    }
}
