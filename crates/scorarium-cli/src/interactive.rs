use std::io::Write;
use std::path::PathBuf;

use reedline::{
    DefaultHinter, DefaultPrompt, DefaultPromptSegment, FileBackedHistory, Reedline, Signal,
};
use scorarium_archive::Archive;

use crate::command::{self, Flow};
use crate::log::LogWriter;

const HIST_SIZE: usize = 1000;

/// Read and run commands from a line editor until `quit` or ctrl-d on an empty line.
pub(crate) async fn run(
    archive: &Archive,
    history: Option<PathBuf>,
    log: &LogWriter,
) -> eyre::Result<()> {
    let mut editor = Reedline::create()
        .with_hinter(Box::new(DefaultHinter::default()))
        .with_external_printer(log.take_printer());
    if let Some(path) = history {
        let history = FileBackedHistory::with_file(HIST_SIZE, path)?;
        editor = editor.with_history(Box::new(history));
    }
    let mut prompt = DefaultPrompt::new(
        DefaultPromptSegment::Basic("scorarium".into()),
        DefaultPromptSegment::Empty,
    );
    let mut stdout = std::io::stdout();
    loop {
        let signal;
        log.set_prompt_active(true);
        (editor, prompt, signal) = tokio::task::spawn_blocking(move || {
            let signal = editor.read_line(&prompt);
            (editor, prompt, signal)
        })
        .await?;
        log.set_prompt_active(false);
        match signal? {
            Signal::Success(line) => match command::execute(archive, &line, &mut stdout).await {
                Ok(Flow::Continue) => stdout.flush()?,
                Ok(Flow::Quit) => break,
                Err(error) => eprintln!("{error:#}"),
            },
            Signal::CtrlD => break,
            // ctrl-c has already cleared the line
            _ => {}
        }
    }
    Ok(())
}
