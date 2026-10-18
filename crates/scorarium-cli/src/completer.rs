use std::sync::Arc;

use clap::{Command, CommandFactory};
use reedline::{Completer, CompletionResult, Span, Suggestion};
use scorarium_engine::archive::Archive;
use tokio::runtime::Handle;

use crate::command::{LIBRARY_COMMANDS_WITHOUT_SUBJECT, Line};
use crate::library::visibility;

/// The clap command tree
fn tree() -> Command {
    let mut tree = Line::command();
    tree.build();
    tree
}

/// Describe where a walk down the command tree stopped.
struct Location<'a> {
    /// The command reached.
    command: &'a Command,
    /// How many words the walk took.
    depth: usize,
    /// Whether the reached command itself took a subject.
    has_subject: bool,
}

/// Follow `words` down the tree, stepping over one subject per command that takes one.
fn locate<'a>(tree: &'a Command, words: &[String]) -> Location<'a> {
    let mut command = tree;
    let mut depth = 0;
    let mut has_subject = false;
    for word in words {
        if let Some(sub) = command.find_subcommand(word) {
            command = sub;
            has_subject = false;
        } else if !has_subject
            && command.has_subcommands()
            && command.get_positionals().next().is_some()
        {
            has_subject = true;
        } else {
            break;
        }
        depth += 1;
    }
    Location {
        command,
        depth,
        has_subject,
    }
}

pub(crate) struct ShellCompleter {
    tree: Command,
    archive: Arc<Archive>,
    handle: Handle,
    selected: Option<i64>,
}

impl ShellCompleter {
    /// Complete against `archive`, rewriting lines for the `selected` library as the shell does.
    ///
    /// `handle` runs archive queries, so the completer must be called off the runtime's threads.
    pub(crate) fn new(archive: Arc<Archive>, handle: Handle, selected: Option<i64>) -> Self {
        ShellCompleter {
            tree: tree(),
            archive,
            handle,
            selected,
        }
    }

    /// List the completions for the word after `words` that start with `typed`.
    fn candidates(&self, words: &[String], typed: &str) -> Vec<(String, Option<String>)> {
        let Location {
            command,
            depth,
            has_subject,
        } = locate(&self.tree, words);
        if !command.has_subcommands() {
            let Some(arg) = command.get_positionals().nth(words.len() - depth) else {
                return Vec::new();
            };
            return arg
                .get_possible_values()
                .iter()
                .filter(|value| value.get_name().starts_with(typed))
                .map(|value| {
                    let help = value.get_help().map(ToString::to_string);
                    (value.get_name().to_string(), help)
                })
                .collect();
        }
        if depth < words.len() {
            return Vec::new();
        }
        let is_library = command.get_name() == "library";
        let mut candidates = if is_library && !has_subject {
            self.libraries(typed)
        } else {
            Vec::new()
        };
        let subcommands = command
            .get_subcommands()
            .filter(|sub| sub.get_name().starts_with(typed))
            .filter(|sub| {
                let name = sub.get_name();
                // Clap's generated help fits neither position
                if !is_library {
                    true
                } else if has_subject {
                    name != "help" && !LIBRARY_COMMANDS_WITHOUT_SUBJECT.contains(&name)
                } else {
                    LIBRARY_COMMANDS_WITHOUT_SUBJECT.contains(&name)
                }
            })
            .map(|sub| {
                let about = sub.get_about().map(ToString::to_string);
                (sub.get_name().to_string(), about)
            });
        candidates.extend(subcommands);
        candidates
    }

    /// List the libraries whose names start with `typed`, quoted for the shell.
    fn libraries(&self, typed: &str) -> Vec<(String, Option<String>)> {
        // There is nowhere to show an error mid-keystroke
        let Ok(libraries) = self.handle.block_on(self.archive.libraries()) else {
            return Vec::new();
        };
        // The shell resolves library names regardless of case
        let typed = typed.to_lowercase();
        libraries
            .into_iter()
            .filter(|library| library.name.to_lowercase().starts_with(&typed))
            .filter_map(|library| {
                let name = shlex::try_quote(&library.name).ok()?.into_owned();
                let description = format!("id {}, {}", library.id, visibility(library.private));
                Some((name, Some(description)))
            })
            .collect()
    }
}

impl Completer for ShellCompleter {
    fn complete(&mut self, line: &str, pos: usize) -> CompletionResult {
        let head = &line[..pos];
        let (before, typed) = head.rsplit_once(char::is_whitespace).unwrap_or(("", head));
        // An unterminated quote before the cursor means the cursor is inside a value
        let Some(words) = shlex::split(before) else {
            return CompletionResult::fresh(Vec::new());
        };
        let rewrite = self.selected.filter(|_| {
            words
                .first()
                .is_none_or(|w| self.tree.find_subcommand(w).is_none())
        });
        let mut candidates = self.candidates(&words, typed);
        if let Some(id) = rewrite {
            let words: Vec<String> = ["library".to_string(), id.to_string()]
                .into_iter()
                .chain(words)
                .collect();
            for candidate in self.candidates(&words, typed) {
                if !candidates.iter().any(|(name, _)| *name == candidate.0) {
                    candidates.push(candidate);
                }
            }
        }
        let start = pos - typed.len();
        let suggestions: Vec<Suggestion> = candidates
            .into_iter()
            .map(|(value, description)| Suggestion {
                value,
                description,
                span: Span::new(start, pos),
                append_whitespace: true,
                ..Suggestion::default()
            })
            .collect();
        CompletionResult::fresh(suggestions)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn completion_follows_the_grammar() {
        let archive = Arc::new(Archive::in_memory().await.unwrap());
        archive.populate_demo().await.unwrap();
        let handle = Handle::current();
        tokio::task::spawn_blocking(move || {
            let values = |selected: Option<i64>, line: &str| -> Vec<(String, usize)> {
                ShellCompleter::new(Arc::clone(&archive), handle.clone(), selected)
                    .complete(line, line.len())
                    .suggestions()
                    .iter()
                    .map(|s| (s.value.clone(), s.span.start))
                    .collect()
            };
            let expected = |names: &[&str], start: usize| -> Vec<(String, usize)> {
                names.iter().map(|name| (name.to_string(), start)).collect()
            };
            assert_eq!(values(None, "library l"), expected(&["list"], 8));
            assert_eq!(
                values(None, "library create 'my lib' p"),
                expected(&["public", "private"], 24)
            );
            assert_eq!(values(None, "library S"), expected(&["'Sheet music'"], 8));
            assert_eq!(values(None, "library b"), expected(&["Books"], 8));
            assert_eq!(
                values(None, "library books "),
                expected(
                    &[
                        "show",
                        "rename",
                        "delete",
                        "select",
                        "publication",
                        "work",
                        "person",
                        "tag"
                    ],
                    14
                )
            );
            assert_eq!(
                values(Some(1), "p"),
                expected(&["publication", "person"], 0)
            );
        })
        .await
        .unwrap();
    }
}
