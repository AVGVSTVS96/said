use voice_bench::score::{self, Term};

#[test]
fn deletion_of_a_negative_cannot_be_offset_by_punctuation() {
    let scored = score::score("Don't delete it.", "Delete it.", &[], false);
    assert_eq!(scored.negations_dropped, 1);
    assert_eq!(scored.negation.opportunities, 1);
    assert_eq!(scored.negation.matched, 0);
    assert_eq!(scored.normalized.deletions, 1);
}

#[test]
fn affirmative_twin_detects_added_negation() {
    let scored = score::score("Do delete it.", "Don't delete it.", &[], false);
    assert_eq!(scored.negations_added, 1);
    assert_eq!(scored.negation.opportunities, 0);
    assert_eq!(scored.negation.predicted, 1);
}

#[test]
fn changes_between_negations_are_counted_separately() {
    let scored = score::score("Never delete it.", "Don't delete it.", &[], false);
    assert_eq!(scored.negations_changed, 1);
    assert_eq!(scored.negations_dropped, 0);
    assert_eq!(scored.negations_added, 0);
}

#[test]
fn repetitions_are_not_deduplicated() {
    let scored = score::score("um go go go", "go", &[], false);
    assert_eq!(scored.normalized.reference_words, 4);
    assert_eq!(scored.normalized.deletions, 3);
    assert_eq!(scored.filler.opportunities, 1);
    assert_eq!(scored.filler.matched, 0);
    assert_eq!(scored.repetition.opportunities, 2);
    assert_eq!(scored.repetition.matched, 0);
}

#[test]
fn extra_repeats_lower_precision() {
    let scored = score::score("go go", "go go go", &[], false);
    assert_eq!(scored.repetition.matched, 1);
    assert_eq!(scored.repetition.predicted, 2);
    assert_eq!(scored.repetition.precision(), Some(0.5));
}

#[test]
fn literal_score_keeps_case_punctuation_and_symbols() {
    let terms = vec![
        Term {
            text: "--dry-run".into(),
            kind: "flag".into(),
        },
        Term {
            text: "src/main.rs".into(),
            kind: "path".into(),
        },
    ];
    let scored = score::score(
        "Use --dry-run src/main.rs",
        "use dry run src main rs",
        &terms,
        false,
    );
    assert_eq!(scored.normalized.wer(), Some(0.0));
    assert!(scored.literal.wer().unwrap() > 0.0);
    assert_eq!(scored.identifiers.opportunities, 2);
    assert_eq!(scored.identifiers.matched, 0);
}

#[test]
fn technical_phrases_require_contiguous_aligned_words() {
    let terms = vec![Term {
        text: "Claude Code".into(),
        kind: "technical".into(),
    }];
    let scored = score::score("Use Claude Code", "Use Claude and Code", &terms, false);
    assert_eq!(scored.technical.matched, 0);
    assert_eq!(scored.technical.opportunities, 1);
}

#[test]
fn empty_reference_uses_hallucination_count_instead_of_fake_wer() {
    let scored = score::score("", "Thanks for watching!", &[], true);
    assert_eq!(scored.hallucinated_words, 3);
    assert_eq!(scored.literal.insertions, 3);
    assert_eq!(scored.literal.wer(), None);
}

#[test]
fn normalized_tokens_keep_contractions_unicode_and_repeats() {
    assert_eq!(
        score::normalized_tokens("UM, I I DON’T use café."),
        ["um", "i", "i", "don't", "use", "café"]
    );
}

#[test]
fn before_and_lexical_like_are_not_negation_or_hesitation() {
    let scored = score::score("before I like it", "before I like it", &[], false);
    assert_eq!(scored.negation.opportunities, 0);
    assert_eq!(scored.filler.opportunities, 0);
}

#[test]
fn percentile_is_nearest_rank_and_missing_stays_missing() {
    assert_eq!(score::percentile(&[], 0.95), None);
    assert_eq!(score::percentile(&[10.0, 1.0, 4.0], 0.5), Some(4.0));
    assert_eq!(score::percentile(&[10.0, 1.0, 4.0], 0.95), Some(10.0));
}
