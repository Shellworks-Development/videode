//! Fuzzy matching for the command palette: the query's characters must
//! appear in order; consecutive runs and word starts score higher.

#[derive(Clone, Debug, PartialEq)]
pub struct Match {
    pub score: i32,
    /// Character (not byte) indices of matched characters in the candidate.
    pub positions: Vec<usize>,
}

pub fn fuzzy(query: &str, candidate: &str) -> Option<Match> {
    let query: Vec<char> = query
        .chars()
        .filter(|c| !c.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect();
    if query.is_empty() {
        return Some(Match {
            score: 0,
            positions: Vec::new(),
        });
    }
    let chars: Vec<char> = candidate.chars().collect();
    let lower: Vec<char> = chars
        .iter()
        .map(|c| c.to_lowercase().next().unwrap_or(*c))
        .collect();
    let mut positions = Vec::with_capacity(query.len());
    let mut score = 0;
    let mut qi = 0;
    for (i, &c) in lower.iter().enumerate() {
        if qi < query.len() && c == query[qi] {
            let word_start = i == 0
                || !chars[i - 1].is_alphanumeric()
                || (chars[i].is_uppercase() && chars[i - 1].is_lowercase());
            score += 1;
            if word_start {
                score += 8;
            }
            if positions.last() == Some(&(i.wrapping_sub(1))) {
                score += 5;
            }
            positions.push(i);
            qi += 1;
        }
    }
    if qi < query.len() {
        return None;
    }
    // Prefer matches that start early and candidates that are short.
    score -= positions[0] as i32;
    score -= (chars.len() as i32) / 8;
    Some(Match { score, positions })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_in_order_only() {
        assert!(fuzzy("spl", "Split at playhead").is_some());
        assert!(fuzzy("lps", "Split").is_none());
        assert_eq!(fuzzy("", "anything").unwrap().score, 0);
    }

    #[test]
    fn word_starts_and_runs_rank_higher() {
        let a = fuzzy("rd", "Ripple delete").unwrap();
        let b = fuzzy("rd", "Redo").unwrap();
        assert!(a.score > b.score, "{a:?} vs {b:?}");
        let tight = fuzzy("grade", "Grade").unwrap();
        let loose = fuzzy("grade", "Go to start and end").map_or(i32::MIN, |m| m.score);
        assert!(tight.score > loose);
        assert_eq!(fuzzy("sp", "Split").unwrap().positions, vec![0, 1]);
    }

    #[test]
    fn case_and_unicode_safe() {
        assert!(fuzzy("PART", "“part two — emergence”").is_some());
        assert_eq!(fuzzy("é", "Café").unwrap().positions, vec![3]);
    }
}
