use clap::{Arg, Command};
use reedline::{Completer, CompletionResult, Span, Suggestion};
use scorarium_engine::Engine;
use scorarium_engine::entity::AnyEntity;
use scorarium_engine::library::LibraryId;
use scorarium_engine::suggest::{SuggestField, Suggested};
use tokio::runtime::Handle;

use crate::command::{self, VERBS_WITHOUT_SUBJECT};
use crate::library::{self, visibility};
use crate::search::context;

/// Describe where a walk down the command tree stopped.
struct Location<'a> {
    /// The command reached.
    command: &'a Command,
    /// How many words the walk took.
    depth: usize,
    /// Whether the reached command itself took a subject.
    has_subject: bool,
    /// The subjects stepped over, in path order.
    subjects: Vec<&'a str>,
}

/// Follow `words` down the tree, stepping over one subject per command that takes one.
fn locate<'a>(tree: &'a Command, words: &'a [String]) -> Location<'a> {
    let mut command = tree;
    let mut depth = 0;
    let mut has_subject = false;
    let mut subjects = Vec::new();
    for word in words {
        if let Some(sub) = command.find_subcommand(word) {
            command = sub;
            has_subject = false;
        } else if !has_subject
            && command.has_subcommands()
            && command.get_positionals().next().is_some()
        {
            has_subject = true;
            subjects.push(word.as_str());
        } else {
            break;
        }
        depth += 1;
    }
    Location {
        command,
        depth,
        has_subject,
        subjects,
    }
}

/// Map a slot's value name to the suggestion field
fn field_for(value_name: &str) -> Option<SuggestField> {
    match value_name {
        "publication" => Some(SuggestField::Publication),
        "work" => Some(SuggestField::Work),
        "person" => Some(SuggestField::Person),
        "tag" => Some(SuggestField::Tag),
        "role" => Some(SuggestField::Role),
        _ => None,
    }
}

pub(crate) struct ShellCompleter {
    tree: Command,
    engine: Engine,
    handle: Handle,
    selected: Option<LibraryId>,
}

impl ShellCompleter {
    /// Complete against `engine`, rewriting lines for the `selected` library as the shell does.
    ///
    /// `handle` runs engine queries, so the completer must be called off the runtime's threads.
    pub(crate) fn new(engine: Engine, handle: Handle, selected: Option<LibraryId>) -> Self {
        ShellCompleter {
            tree: command::tree(),
            engine,
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
            subjects,
        } = locate(&self.tree, words);
        // Every slot with a field sits below a library, which is the first subject
        let slot = |arg: &Arg| {
            let field = field_for(arg.get_value_names()?.first()?)?;
            Some((*subjects.first()?, field))
        };
        if !command.has_subcommands() {
            let option = words
                .last()
                .and_then(|word| word.strip_prefix("--"))
                .and_then(|long| command.get_opts().find(|arg| arg.get_long() == Some(long)));
            let Some(arg) = option.or_else(|| command.get_positionals().nth(words.len() - depth))
            else {
                return Vec::new();
            };
            if let Some((library, field)) = slot(arg) {
                return self.suggestions(library, field, typed);
            }
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
        let takes_subject = command.has_subcommands() && command.get_positionals().next().is_some();
        let mut candidates = if has_subject {
            Vec::new()
        } else if is_library {
            self.libraries(typed)
        } else {
            command
                .get_positionals()
                .next()
                .and_then(slot)
                .map_or_else(Vec::new, |(library, field)| {
                    self.suggestions(library, field, typed)
                })
        };
        let subcommands = command
            .get_subcommands()
            .filter(|sub| sub.get_name().starts_with(typed))
            .filter(|sub| {
                let name = sub.get_name();
                // Clap's generated help fits neither position
                if !takes_subject {
                    true
                } else if has_subject {
                    name != "help" && !VERBS_WITHOUT_SUBJECT.contains(&name)
                } else {
                    VERBS_WITHOUT_SUBJECT.contains(&name)
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
        let Ok(libraries) = self.handle.block_on(self.engine.libraries()) else {
            return Vec::new();
        };
        // The shell resolves library names regardless of case
        let typed = typed.to_lowercase();
        libraries
            .into_iter()
            .filter(|library| library.name().to_lowercase().starts_with(&typed))
            .filter_map(|library| {
                let name = shlex::try_quote(library.name()).ok()?.into_owned();
                let description =
                    format!("id {}, {}", library.id(), visibility(library.is_private()));
                Some((name, Some(description)))
            })
            .collect()
    }

    fn suggestions(
        &self,
        library: &str,
        field: SuggestField,
        typed: &str,
    ) -> Vec<(String, Option<String>)> {
        let found = self.handle.block_on(async {
            let library = library::resolve(&self.engine, library).await?;
            eyre::Ok(library.suggest(field, typed, 10).await?)
        });
        let Ok(found) = found else {
            return Vec::new();
        };
        found
            .into_iter()
            .filter_map(|suggestion| {
                let (value, description) = match suggestion.item {
                    Suggested::Publication(publication) => {
                        let title = publication.title().unwrap_or_default().to_string();
                        (title, Some(context(&AnyEntity::Publication(publication))))
                    }
                    Suggested::Work { work, .. } => {
                        let title = work.title().to_string();
                        (title, Some(context(&AnyEntity::Work(work))))
                    }
                    Suggested::Person(person) => {
                        let name = person.name().to_string();
                        (name, Some(context(&AnyEntity::Person(person))))
                    }
                    Suggested::Tag { tag, count } => {
                        (tag.to_string(), Some(format!("{count} tagged")))
                    }
                    Suggested::Role(role) => (role, None),
                    _ => return None,
                };
                let value = shlex::try_quote(&value).ok()?.into_owned();
                Some((value, description))
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
    use scorarium_engine::EngineOptions;

    use super::*;

    #[tokio::test]
    async fn completion_follows_the_grammar() {
        let engine = Engine::demo(EngineOptions::default()).await.unwrap();
        let handle = Handle::current();
        tokio::task::spawn_blocking(move || {
            let values = |selected: Option<LibraryId>, line: &str| -> Vec<(String, usize)> {
                ShellCompleter::new(engine.clone(), handle.clone(), selected)
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
                values(Some("1".parse().unwrap()), "p"),
                expected(&["publication", "person"], 0)
            );
            assert_eq!(
                values(None, "library books publication P"),
                expected(&["'Practical Vim'", "'Pro Git'"], 26)
            );
            assert_eq!(
                values(None, "library books publication 'Pro Git' "),
                expected(&["show", "audit"], 36)
            );
            assert_eq!(
                values(None, "library 'sheet music' publication list --tag r").first(),
                Some(&("russian".to_string(), 45))
            );
        })
        .await
        .unwrap();
    }
}
