use frizbee::{CaseMatching, Config, Matcher, SortStrategy, UnicodeMatching};

/// Normalize unicode text with punctuation into search tokens
pub(crate) fn normalize(text: &str) -> String {
    let ascii = deunicode::deunicode(text).to_ascii_lowercase();
    let mut normalized = String::with_capacity(ascii.len());
    for word in ascii
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty())
    {
        if !normalized.is_empty() {
            normalized.push(' ');
        }
        normalized.push_str(word);
    }
    normalized
}

/// How many characters of a typed word may be absent from the word it matches
fn typo_budget(word: &str) -> u16 {
    match word.len() {
        0..=3 => 0,
        4..=7 => 1,
        _ => 2,
    }
}

/// Return indices of the candidates that fuzzily match the typed text, best first.
pub(crate) fn rank<S: AsRef<str>>(typed: &str, candidates: &[S]) -> Vec<usize> {
    let typed = normalize(typed);
    if typed.is_empty() {
        return Vec::new();
    }
    let normalized: Vec<String> = candidates
        .iter()
        .map(|candidate| normalize(candidate.as_ref()))
        .collect();
    // Every word of every candidate in one slice, so frizbee makes a single pass per typed word
    let (words, owners): (Vec<&str>, Vec<usize>) = normalized
        .iter()
        .enumerate()
        .flat_map(|(i, text)| text.split(' ').map(move |word| (word, i)))
        .unzip();
    let mut totals: Vec<Option<u32>> = vec![Some(0); candidates.len()];
    for needle in typed.split(' ') {
        let config = Config {
            max_typos: Some(typo_budget(needle)),
            casing: CaseMatching::Ignore,
            unicode: UnicodeMatching::Ignore,
            sort: SortStrategy::IndexAsc,
            ..Config::default()
        };
        let mut best: Vec<Option<u16>> = vec![None; candidates.len()];
        for m in Matcher::new(needle, &config).match_list(&words) {
            let slot = &mut best[owners[m.index as usize]];
            *slot = Some(slot.map_or(m.score, |b| b.max(m.score)));
        }
        for (total, best) in totals.iter_mut().zip(best) {
            *total = total.zip(best).map(|(t, b)| t + u32::from(b));
        }
    }
    let mut hits: Vec<(usize, u32)> = totals
        .into_iter()
        .enumerate()
        .filter_map(|(i, total)| Some((i, total?)))
        .collect();
    hits.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    hits.into_iter().map(|(i, _)| i).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranks_by_how_well_every_typed_word_matches() {
        assert_eq!(
            normalize("Saint-Saëns: Op. 27, No. 2"),
            "saint saens op 27 no 2"
        );

        let names = [
            "Moonlight Sonata",
            "Sonata in C",
            "Snake Charmer",
            "Nocturne",
            "Nocturnes",
            "Sergei Rachmaninoff",
            "Camille Saint-Saëns",
            "Antonín Dvořák",
        ];
        // Contiguous letters first, then subsequence hits in load order
        assert_eq!(rank("sna", &names), [2, 0, 1]);
        // Every word must match, in any order
        assert_eq!(rank("sonata moonlight", &names), [0]);
        // Nine letters allow one absent, so the plural finds the singular too
        assert_eq!(rank("nocturnes", &names), [4, 3]);
        assert_eq!(rank("Rachmaninov", &names), [5]);
        assert_eq!(rank("dvorak", &names), [7]);
        assert_eq!(rank("saint saens", &names), [6]);
        // A typed word cannot spread across two candidate words
        assert!(rank("saintsaens", &names).is_empty());
        assert!(rank("erx", &names).is_empty());
        assert!(rank("  ", &names).is_empty());
    }
}
