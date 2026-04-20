//! Text-similarity primitives used by the v1.6 stagnation detector.
//!
//! The v1.5 detector only looked at `missing[]` count. As a result, a session
//! could be killed the moment two agents deadlocked at the same count even if
//! one of them was on a genuinely different problem each round — see the
//! critique in PRD decision #88 (v1.6 stagnation redesign).
//!
//! The v1.6 detector needs to answer: "are the agents still yelling about the
//! SAME items across rounds?" — i.e. condition B in the A+B+¬C guard. This
//! module provides the similarity primitive.
//!
//! Design choices:
//!
//! - **Tokenizer is intentionally dumb.** Lowercase ASCII, strip ASCII
//!   punctuation, split on whitespace, drop single-`char` tokens. No stop-word
//!   list — sessions can be Chinese / Japanese / mixed, and a fixed English
//!   stop-word list would silently degrade cross-round matching on non-English
//!   text. CJK script lands as per-character tokens, which is good enough for
//!   Jaccard on missing-item titles.
//! - **No stemming, no embeddings.** Runs locally on user metal; we pay no
//!   startup cost and introduce no model dependency for what is a heuristic
//!   guard.
//! - **Pairwise list match.** `lists_substantially_same` asks: does ≥80% of
//!   each list have a partner in the other via Jaccard ≥ threshold? This is
//!   symmetric so that "Codex added one new missing item" and "Codex dropped
//!   one missing item" are both treated as content drift (→ B fails).

use std::collections::HashSet;

/// Tokenize for cross-round similarity. See module doc for why this is
/// intentionally lightweight. Exposed for tests.
pub(crate) fn tokenize(s: &str) -> HashSet<String> {
    s.chars()
        .map(|c| {
            if c.is_ascii_punctuation() {
                ' '
            } else if c.is_ascii_uppercase() {
                c.to_ascii_lowercase()
            } else {
                c
            }
        })
        .collect::<String>()
        .split_whitespace()
        .filter(|t| t.chars().count() > 1)
        .map(|t| t.to_string())
        .collect()
}

/// Jaccard similarity between the token sets of `a` and `b`.
/// Both empty → 1.0 (trivially "same"). One empty, one non-empty → 0.0.
pub fn jaccard(a: &str, b: &str) -> f64 {
    let ta = tokenize(a);
    let tb = tokenize(b);
    if ta.is_empty() && tb.is_empty() {
        return 1.0;
    }
    if ta.is_empty() || tb.is_empty() {
        return 0.0;
    }
    let inter = ta.intersection(&tb).count() as f64;
    let union = ta.union(&tb).count() as f64;
    inter / union
}

/// Default similarity threshold the stagnation detector uses.
///
/// Calibrated against real v1.5 Hermes-Linux session missing-items:
/// close rewordings (shared subject phrase + some shared descriptor) tend to
/// land at 0.35–0.5; single-word-overlap false neighbors like `"Docker
/// missing"` vs `"Codex missing"` are at 1/3 ≈ 0.33. Threshold 0.35 is the
/// first value that keeps `Docker/Codex` safely below while letting
/// `ISO artifact still missing` / `ISO not yet built end-to-end` through.
///
/// Errs **toward false negatives** — we'd rather miss a true rewording
/// pairing (→ B fails → session keeps running) than wrongly declare two
/// distinct problems "the same" (→ B true → risk of unjust kill).
pub const DEFAULT_THRESHOLD: f64 = 0.35;

/// Fraction of each list that must have a pair on the other side before two
/// lists are considered "substantially the same". 0.8 tolerates one agent
/// splitting or merging an item (e.g. `["docker missing", "ISO unbuilt"]`
/// versus `["ISO+Docker pipeline unfinished"]`) without declaring content
/// drift, but catches a real new item appearing.
const PAIR_COVERAGE: f64 = 0.8;

/// Returns true when `curr` and `prev` missing-item lists represent the same
/// underlying issues (per Jaccard ≥ `threshold` and pair coverage ≥ 0.8
/// in both directions).
pub fn lists_substantially_same(curr: &[String], prev: &[String], threshold: f64) -> bool {
    // Both empty is trivially "same"; exactly one empty is a genuine delta.
    if curr.is_empty() && prev.is_empty() {
        return true;
    }
    if curr.is_empty() || prev.is_empty() {
        return false;
    }
    let curr_matched = curr
        .iter()
        .filter(|c| prev.iter().any(|p| jaccard(c, p) >= threshold))
        .count();
    let prev_matched = prev
        .iter()
        .filter(|p| curr.iter().any(|c| jaccard(c, p) >= threshold))
        .count();
    let curr_rate = curr_matched as f64 / curr.len() as f64;
    let prev_rate = prev_matched as f64 / prev.len() as f64;
    curr_rate >= PAIR_COVERAGE && prev_rate >= PAIR_COVERAGE
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn jaccard_identical() {
        assert_eq!(jaccard("same string", "same string"), 1.0);
    }

    #[test]
    fn jaccard_empty_vs_empty_is_one() {
        // Both empty is trivially equal — saves callers from special-casing
        // "both goals fully met" rounds.
        assert_eq!(jaccard("", ""), 1.0);
    }

    #[test]
    fn jaccard_empty_vs_nonempty_is_zero() {
        assert_eq!(jaccard("", "something"), 0.0);
        assert_eq!(jaccard("something", ""), 0.0);
    }

    #[test]
    fn jaccard_disjoint_zero() {
        assert_eq!(jaccard("apples oranges", "kittens puppies"), 0.0);
    }

    #[test]
    fn jaccard_case_and_punctuation_insensitive() {
        assert_eq!(
            jaccard("Docker is missing.", "docker IS missing"),
            1.0,
            "case + trailing period should not matter"
        );
    }

    #[test]
    fn jaccard_rewording_scores_above_threshold() {
        // Pulled from a real Hermes-Linux v1.5 session. Codex kept raising
        // this same underlying problem in slightly different words each
        // round; we want the detector to recognize it as repeated content.
        let a = "Full end-to-end ISO artifact is still missing";
        let b = "ISO artifact not yet built end-to-end";
        let score = jaccard(a, b);
        assert!(
            score >= DEFAULT_THRESHOLD,
            "rewording should clear threshold; got {score}"
        );
    }

    #[test]
    fn jaccard_single_word_overlap_stays_below_threshold() {
        // The classic false-neighbor pair: two unrelated items that both end
        // in "missing". One shared token out of three → 0.33, and we want
        // the threshold set JUST ABOVE this so unrelated items with one
        // incidentally-shared keyword never pair.
        let score = jaccard("Docker missing", "Codex missing");
        assert!(score < DEFAULT_THRESHOLD, "got {score}");
    }

    #[test]
    fn tokenize_strips_single_chars() {
        // Single-char tokens are noise (stop-word-adjacent).
        let s = tokenize("a b ci/cd is down");
        assert!(!s.contains("a"));
        assert!(!s.contains("b"));
        assert!(s.contains("ci"));
        assert!(s.contains("cd"));
        assert!(s.contains("is"));
        assert!(s.contains("down"));
    }

    #[test]
    fn lists_same_trivial() {
        assert!(lists_substantially_same(
            &v(&["a missing", "b broken"]),
            &v(&["a missing", "b broken"]),
            DEFAULT_THRESHOLD,
        ));
    }

    #[test]
    fn lists_same_survives_light_rewording() {
        // Realistic close-rewording of the same items across rounds. We pair
        // whole-title items here (not sub-phrase rewordings like
        // "unavailable" vs "not available", which Jaccard on bare tokens is
        // too blunt to catch — that's an accepted false negative per the
        // threshold doc).
        let curr = v(&[
            "ISO artifact not yet built end-to-end",
            "systemd heartbeat unit still absent",
        ]);
        let prev = v(&[
            "Full end-to-end ISO artifact is still missing",
            "systemd heartbeat unit missing",
        ]);
        assert!(lists_substantially_same(&curr, &prev, DEFAULT_THRESHOLD));
    }

    #[test]
    fn lists_flip_when_new_item_appears() {
        // r1 had 1 item; r2 still has that + a genuinely new one. Prev-side
        // coverage is fine (the one item paired), but curr-side coverage is
        // 1/2 = 0.5 which is below 0.8 → content drifted.
        let prev = v(&["Docker missing"]);
        let curr = v(&["Docker missing", "Web UI has no /health endpoint"]);
        assert!(!lists_substantially_same(&curr, &prev, DEFAULT_THRESHOLD));
    }

    #[test]
    fn lists_flip_when_item_dropped() {
        // Mirror of the above — prev had 2 items, curr has 1 paired; prev-side
        // coverage falls to 0.5.
        let prev = v(&["Docker missing", "Web UI broken"]);
        let curr = v(&["Docker still missing"]);
        assert!(!lists_substantially_same(&curr, &prev, DEFAULT_THRESHOLD));
    }

    #[test]
    fn lists_different_return_false() {
        let curr = v(&["Docker missing"]);
        let prev = v(&["TCC permissions broken"]);
        assert!(!lists_substantially_same(&curr, &prev, DEFAULT_THRESHOLD));
    }

    #[test]
    fn lists_empty_vs_empty_same() {
        assert!(lists_substantially_same(&[], &[], DEFAULT_THRESHOLD));
    }

    #[test]
    fn lists_empty_vs_nonempty_differ() {
        assert!(!lists_substantially_same(
            &v(&["something"]),
            &[],
            DEFAULT_THRESHOLD,
        ));
    }
}
