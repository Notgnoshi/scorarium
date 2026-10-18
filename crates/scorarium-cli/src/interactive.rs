use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;

use reedline::{
    ColumnarMenu, DefaultHinter, DefaultPrompt, DefaultPromptSegment, Emacs, FileBackedHistory,
    KeyCode, KeyModifiers, MenuBuilder, Reedline, ReedlineEvent, ReedlineMenu, Signal,
    default_emacs_keybindings,
};
use scorarium_engine::archive::Archive;
use tokio::runtime::Handle;

use crate::command::{Flow, Shell};
use crate::completer::ShellCompleter;
use crate::log::LogWriter;

const HIST_SIZE: usize = 1000;

/// Read and run commands from a line editor until `quit` or ctrl-d on an empty line.
pub(crate) async fn run(
    archive: &Arc<Archive>,
    history: Option<PathBuf>,
    log: &LogWriter,
) -> eyre::Result<()> {
    let mut keybindings = default_emacs_keybindings();
    keybindings.add_binding(
        KeyModifiers::NONE,
        KeyCode::Tab,
        ReedlineEvent::UntilFound(vec![
            ReedlineEvent::Menu("completion".into()),
            ReedlineEvent::MenuNext,
        ]),
    );
    let mut editor = Reedline::create()
        .with_hinter(Box::new(DefaultHinter::default()))
        .with_external_printer(log.take_printer())
        .with_menu(ReedlineMenu::EngineCompleter(Box::new(
            ColumnarMenu::default().with_name("completion"),
        )))
        .with_edit_mode(Box::new(Emacs::new(keybindings)));
    if let Some(path) = history {
        let history = FileBackedHistory::with_file(HIST_SIZE, path)?;
        editor = editor.with_history(Box::new(history));
    }
    let mut shell = Shell::new(archive);
    let mut stdout = std::io::stdout();
    loop {
        let label = match shell.selected().await? {
            Some(library) => format!("scorarium[{}]", library.name),
            None => "scorarium".to_string(),
        };
        let prompt = DefaultPrompt::new(
            DefaultPromptSegment::Basic(label),
            DefaultPromptSegment::Empty,
        );
        editor = editor.with_completer(Box::new(ShellCompleter::new(
            Arc::clone(archive),
            Handle::current(),
            shell.selection(),
        )));
        let signal;
        log.set_prompt_active(true);
        (editor, signal) = tokio::task::spawn_blocking(move || {
            let signal = editor.read_line(&prompt);
            (editor, signal)
        })
        .await?;
        log.set_prompt_active(false);
        match signal? {
            Signal::Success(line) => match shell.execute(&line, &mut stdout).await {
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
