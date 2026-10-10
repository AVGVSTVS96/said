use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use unicode_normalization::UnicodeNormalization;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Term {
    pub text: String,
    pub kind: String,
}

#[derive(Debug, Default, Clone, Serialize)]
pub struct Retention {
    pub opportunities: usize,
    pub matched: usize,
    pub predicted: usize,
}

impl Retention {
    pub fn recall(&self) -> Option<f64> {
        ratio(self.matched, self.opportunities)
    }

    pub fn precision(&self) -> Option<f64> {
        ratio(self.matched, self.predicted)
    }

    pub fn add(&mut self, other: &Self) {
        self.opportunities += other.opportunities;
        self.matched += other.matched;
        self.predicted += other.predicted;
    }
}

#[derive(Debug, Default, Clone, Serialize)]
pub struct Edits {
    pub reference_words: usize,
    pub hypothesis_words: usize,
    pub substitutions: usize,
    pub deletions: usize,
    pub insertions: usize,
}

impl Edits {
    pub fn wer(&self) -> Option<f64> {
        ratio(
            self.substitutions + self.deletions + self.insertions,
            self.reference_words,
        )
    }

    pub fn add(&mut self, other: &Self) {
        self.reference_words += other.reference_words;
        self.hypothesis_words += other.hypothesis_words;
        self.substitutions += other.substitutions;
        self.deletions += other.deletions;
        self.insertions += other.insertions;
    }
}

#[derive(Debug, Default, Serialize)]
pub struct Score {
    pub literal: Edits,
    pub normalized: Edits,
    pub negation: Retention,
    pub negations_dropped: usize,
    pub negations_added: usize,
    pub negations_changed: usize,
    pub filler: Retention,
    pub repetition: Retention,
    pub technical: Retention,
    pub identifiers: Retention,
    pub hallucinated_words: usize,
}

#[derive(Debug, Serialize)]
pub struct Alignment {
    pub reference: Option<usize>,
    pub hypothesis: Option<usize>,
    pub equal: bool,
}

pub fn normalized_tokens(text: &str) -> Vec<String> {
    let text: String = text
        .nfc()
        .flat_map(char::to_lowercase)
        .map(|c| if matches!(c, '’' | '‘') { '\'' } else { c })
        .collect();
    text.split(|c: char| !c.is_alphanumeric() && c != '\'')
        .map(|word| word.trim_matches('\''))
        .filter(|word| !word.is_empty())
        .map(String::from)
        .collect()
}

fn negation(word: &str) -> bool {
    matches!(
        word,
        "not" | "never" | "no" | "without" | "neither" | "nor" | "cannot"
    ) || word.ends_with("n't")
}

fn filler(word: &str) -> bool {
    matches!(word, "um" | "uh" | "uhm" | "erm" | "er" | "hmm")
}

fn repeats(words: &[String]) -> BTreeSet<usize> {
    (1..words.len())
        .filter(|&i| words[i] == words[i - 1])
        .collect()
}

pub fn align<T: Eq>(reference: &[T], hypothesis: &[T]) -> Vec<Alignment> {
    let width = hypothesis.len() + 1;
    let mut back = vec![0_u8; (reference.len() + 1) * width];
    for cell in back.iter_mut().take(width).skip(1) {
        *cell = 3;
    }
    let mut previous: Vec<usize> = (0..width).collect();
    for (i, word) in reference.iter().enumerate() {
        let mut current = vec![i + 1; width];
        back[(i + 1) * width] = 2;
        for (j, other) in hypothesis.iter().enumerate() {
            let diagonal = previous[j] + usize::from(word != other);
            let deletion = previous[j + 1] + 1;
            let insertion = current[j] + 1;
            let cost = diagonal.min(deletion).min(insertion);
            current[j + 1] = cost;
            back[(i + 1) * width + j + 1] = if cost == diagonal {
                1
            } else if cost == deletion {
                2
            } else {
                3
            };
        }
        previous = current;
    }
    let (mut i, mut j) = (reference.len(), hypothesis.len());
    let mut path = Vec::new();
    while i > 0 || j > 0 {
        let (a, b) = match back[i * width + j] {
            1 => {
                i -= 1;
                j -= 1;
                (Some(i), Some(j))
            }
            2 => {
                i -= 1;
                (Some(i), None)
            }
            3 => {
                j -= 1;
                (None, Some(j))
            }
            _ => unreachable!(),
        };
        path.push(Alignment {
            reference: a,
            hypothesis: b,
            equal: a.zip(b).is_some_and(|(a, b)| reference[a] == hypothesis[b]),
        });
    }
    path.reverse();
    path
}

fn edits<T: Eq>(reference: &[T], hypothesis: &[T], path: &[Alignment]) -> Edits {
    let mut counts = Edits {
        reference_words: reference.len(),
        hypothesis_words: hypothesis.len(),
        ..Edits::default()
    };
    for step in path.iter().filter(|step| !step.equal) {
        match (step.reference, step.hypothesis) {
            (Some(_), Some(_)) => counts.substitutions += 1,
            (Some(_), None) => counts.deletions += 1,
            (None, Some(_)) => counts.insertions += 1,
            _ => unreachable!(),
        }
    }
    counts
}

fn retention(
    reference: &BTreeSet<usize>,
    hypothesis: &BTreeSet<usize>,
    path: &[Alignment],
) -> Retention {
    Retention {
        opportunities: reference.len(),
        predicted: hypothesis.len(),
        matched: path
            .iter()
            .filter(|s| {
                s.equal
                    && s.reference.is_some_and(|i| reference.contains(&i))
                    && s.hypothesis.is_some_and(|i| hypothesis.contains(&i))
            })
            .count(),
    }
}

fn term_retention<T: Eq>(
    reference: &[T],
    hypothesis: &[T],
    term: &[T],
    path: &[Alignment],
) -> Retention {
    let occurrences = |words: &[T]| -> Vec<usize> {
        if term.is_empty() {
            Vec::new()
        } else {
            words
                .windows(term.len())
                .enumerate()
                .filter(|(_, w)| *w == term)
                .map(|(i, _)| i)
                .collect()
        }
    };
    let starts = occurrences(reference);
    let mut map = vec![None; reference.len()];
    for step in path.iter().filter(|step| step.equal) {
        map[step.reference.unwrap()] = step.hypothesis;
    }
    let matched = starts
        .iter()
        .filter(|&&start| {
            map[start].is_some_and(|j| (0..term.len()).all(|k| map[start + k] == Some(j + k)))
        })
        .count();
    Retention {
        opportunities: starts.len(),
        predicted: occurrences(hypothesis).len(),
        matched,
    }
}

pub fn score(reference: &str, hypothesis: &str, terms: &[Term], non_speech: bool) -> Score {
    let literal_a: Vec<_> = reference.split_whitespace().collect();
    let literal_b: Vec<_> = hypothesis.split_whitespace().collect();
    let literal_path = align(&literal_a, &literal_b);
    let a = normalized_tokens(reference);
    let b = normalized_tokens(hypothesis);
    let path = align(&a, &b);
    let positions = |words: &[String], predicate: fn(&str) -> bool| -> BTreeSet<usize> {
        words
            .iter()
            .enumerate()
            .filter(|(_, w)| predicate(w))
            .map(|(i, _)| i)
            .collect()
    };
    let mut result = Score {
        literal: edits(&literal_a, &literal_b, &literal_path),
        normalized: edits(&a, &b, &path),
        negation: retention(&positions(&a, negation), &positions(&b, negation), &path),
        filler: retention(&positions(&a, filler), &positions(&b, filler), &path),
        repetition: retention(&repeats(&a), &repeats(&b), &path),
        hallucinated_words: if non_speech { b.len() } else { 0 },
        ..Score::default()
    };
    for step in path.iter().filter(|step| !step.equal) {
        let reference_negation = step.reference.is_some_and(|i| negation(&a[i]));
        let hypothesis_negation = step.hypothesis.is_some_and(|i| negation(&b[i]));
        match (reference_negation, hypothesis_negation) {
            (true, true) => result.negations_changed += 1,
            (true, false) => result.negations_dropped += 1,
            (false, true) => result.negations_added += 1,
            _ => {}
        }
    }
    for term in terms {
        if term.kind == "technical" {
            let words = normalized_tokens(&term.text);
            result.technical.add(&term_retention(&a, &b, &words, &path));
        } else {
            let words: Vec<_> = term.text.split_whitespace().collect();
            result.identifiers.add(&term_retention(
                &literal_a,
                &literal_b,
                &words,
                &literal_path,
            ));
        }
    }
    result
}

pub fn ratio(numerator: usize, denominator: usize) -> Option<f64> {
    (denominator > 0).then(|| numerator as f64 / denominator as f64)
}

pub fn percentile(values: &[f64], fraction: f64) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    Some(
        sorted[((sorted.len() as f64 * fraction).ceil() as usize)
            .saturating_sub(1)
            .min(sorted.len() - 1)],
    )
}
