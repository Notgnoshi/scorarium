use clap::{Command, CommandFactory};
use reedline::{Completer, CompletionResult, Span, Suggestion};

use crate::command::Line;

/// The clap command tree
fn tree() -> Command {
    let mut tree = Line::command();
    tree.build();
    tree
}

/// Follow `words` down the tree as far as they name subcommands.
///
/// Returns the deepest command reached and how many words it took to get there.
fn locate<'a>(tree: &'a Command, words: &[String]) -> (&'a Command, usize) {
    let mut command = tree;
    let mut depth = 0;
    while let Some(sub) = words.get(depth).and_then(|w| command.find_subcommand(w)) {
        command = sub;
        depth += 1;
    }
    (command, depth)
}

pub(crate) struct ShellCompleter {
    tree: Command,
}

impl Default for ShellCompleter {
    fn default() -> Self {
        ShellCompleter { tree: tree() }
    }
}

impl ShellCompleter {
    fn candidates(&self, words: &[String]) -> Vec<(String, Option<String>)> {
        let (command, depth) = locate(&self.tree, words);
        if command.has_subcommands() {
            if depth < words.len() {
                return Vec::new();
            }
            return command
                .get_subcommands()
                .map(|sub| {
                    let about = sub.get_about().map(ToString::to_string);
                    (sub.get_name().to_string(), about)
                })
                .collect();
        }
        let Some(arg) = command.get_positionals().nth(words.len() - depth) else {
            return Vec::new();
        };
        arg.get_possible_values()
            .iter()
            .map(|value| {
                let help = value.get_help().map(ToString::to_string);
                (value.get_name().to_string(), help)
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
        let start = pos - typed.len();
        let suggestions: Vec<Suggestion> = self
            .candidates(&words)
            .into_iter()
            .filter(|(name, _)| name.starts_with(typed))
            .map(|(name, description)| Suggestion {
                value: name,
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

    #[test]
    fn keywords_complete_at_their_position() {
        let mut completer = ShellCompleter::default();
        let mut values = |line: &str| -> Vec<(String, usize)> {
            completer
                .complete(line, line.len())
                .suggestions()
                .iter()
                .map(|s| (s.value.clone(), s.span.start))
                .collect()
        };
        assert_eq!(values("library s"), [("show".to_string(), 8)]);
        assert_eq!(
            values("library create 'my lib' p"),
            [("public".to_string(), 24), ("private".to_string(), 24)]
        );
        assert_eq!(values("library show B"), []);
    }
}
