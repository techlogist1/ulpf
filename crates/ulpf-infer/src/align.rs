//! Sequence alignment over token sequences. Lines are short (tens of tokens), so the
//! quadratic tables are cheap and give count-insensitive alignment: an absent optional
//! field is a gap, not a different template.

/// Plain longest common subsequence: matched index pairs `(i, j)` in order, under
/// `eq(i, j)`. Used for similarity, where only the count matters.
pub fn lcs(n: usize, m: usize, eq: impl Fn(usize, usize) -> bool) -> Vec<(usize, usize)> {
    let w = m + 1;
    let mut dp = vec![0u16; (n + 1) * w];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            dp[i * w + j] = if eq(i, j) { dp[(i + 1) * w + j + 1] + 1 } else { dp[(i + 1) * w + j].max(dp[i * w + j + 1]) };
        }
    }
    let mut out = Vec::with_capacity(dp[0] as usize);
    let (mut i, mut j) = (0, 0);
    while i < n && j < m {
        if eq(i, j) {
            out.push((i, j));
            i += 1;
            j += 1;
        } else if dp[(i + 1) * w + j] >= dp[i * w + j + 1] {
            i += 1;
        } else {
            j += 1;
        }
    }
    out
}

/// Maximum-weight alignment with a gap-open penalty and a substitution state (Gotoh's
/// recurrence over suffixes, so the traceback runs forward and takes the earliest match on
/// a tie). `weight(i, j)` is 0 for no match, higher for a better one: exact constants
/// outweigh slot matches, so a variable slot cannot swallow a keyword (`{user}` aligning
/// with `from`). A run of unmatched tokens costs `GAP_OPEN`, so one contiguous absent
/// field beats the same tokens absent in two pieces (a missing `NAT (...)` block must not
/// pull a line's address pair into the block). Two tokens that `subst` allows to stand in
/// for each other cost `SUB` as a pair, cheaper than a deletion plus an insertion, so
/// `Accepted publickey` against `Failed password` is two word-for-word disagreements with
/// the space between them still aligned, not one opaque region. Only matches are returned.
pub fn align(n: usize, m: usize, weight: impl Fn(usize, usize) -> u16, subst: impl Fn(usize, usize) -> bool) -> Vec<(usize, usize)> {
    const GAP_OPEN: i32 = 2;
    const SUB: i32 = 1;
    const NEG: i32 = i32::MIN / 4;
    let w = m + 1;
    let idx = |i: usize, j: usize| i * w + j;
    // mat: a[i] matched to b[j]; sub: a[i] stands in for b[j]; del: a[i] against a gap;
    // ins: b[j] against a gap. Each holds the best score for the remaining suffixes.
    let mut mat = vec![NEG; (n + 1) * w];
    let mut sub = vec![NEG; (n + 1) * w];
    let mut del = vec![NEG; (n + 1) * w];
    let mut ins = vec![NEG; (n + 1) * w];
    mat[idx(n, m)] = 0;
    for i in (0..n).rev() {
        del[idx(i, m)] = -GAP_OPEN;
    }
    for j in (0..m).rev() {
        ins[idx(n, j)] = -GAP_OPEN;
    }
    let best = |mat: &[i32], sub: &[i32], del: &[i32], ins: &[i32], k: usize| mat[k].max(sub[k]).max(del[k]).max(ins[k]);
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            let diag = best(&mat, &sub, &del, &ins, idx(i + 1, j + 1));
            let here = weight(i, j);
            if here > 0 {
                mat[idx(i, j)] = diag + i32::from(here);
            } else if subst(i, j) {
                sub[idx(i, j)] = diag - SUB;
            }
            let down = idx(i + 1, j);
            del[idx(i, j)] = (mat[down] - GAP_OPEN).max(sub[down] - GAP_OPEN).max(del[down]).max(ins[down] - GAP_OPEN);
            let right = idx(i, j + 1);
            ins[idx(i, j)] = (mat[right] - GAP_OPEN).max(sub[right] - GAP_OPEN).max(ins[right]).max(del[right] - GAP_OPEN);
        }
    }
    // 0 mat, 1 sub, 2 del, 3 ins; ties prefer a match, then a substitution
    let pick = |k: usize| -> u8 {
        let (a, b, c, d) = (mat[k], sub[k], del[k], ins[k]);
        if a >= b && a >= c && a >= d { 0 } else if b >= c && b >= d { 1 } else if c >= d { 2 } else { 3 }
    };
    let next_state = |k: usize, target: i32| -> u8 {
        if mat[k] == target { 0 } else if sub[k] == target { 1 } else if del[k] == target { 2 } else { 3 }
    };
    let mut out = Vec::new();
    let (mut i, mut j) = (0, 0);
    let mut state = pick(idx(0, 0));
    while i < n || j < m {
        match state {
            0 | 1 => {
                let k = idx(i, j);
                let target = if state == 0 {
                    out.push((i, j));
                    mat[k] - i32::from(weight(i, j))
                } else {
                    sub[k] + SUB
                };
                i += 1;
                j += 1;
                state = next_state(idx(i, j), target);
            }
            2 => {
                let v = del[idx(i, j)];
                i += 1;
                let k = idx(i, j);
                state = if del[k] == v { 2 } else if mat[k] - GAP_OPEN == v { 0 } else if sub[k] - GAP_OPEN == v { 1 } else { 3 };
            }
            _ => {
                let v = ins[idx(i, j)];
                j += 1;
                let k = idx(i, j);
                state = if ins[k] == v { 3 } else if mat[k] - GAP_OPEN == v { 0 } else if sub[k] - GAP_OPEN == v { 1 } else { 2 };
            }
        }
        if i == n && j == m {
            break;
        }
        if i == n {
            state = 3;
        } else if j == m {
            state = 2;
        }
    }
    out
}

/// `2·LCS / (|a| + |b|)` over two word lists; 1.0 when both are empty.
pub fn similarity(a: &[&[u8]], b: &[&[u8]]) -> f64 {
    if a.is_empty() && b.is_empty() {
        return 1.0;
    }
    let k = lcs(a.len(), b.len(), |i, j| a[i] == b[j]).len();
    2.0 * k as f64 / (a.len() + b.len()) as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lcs_and_similarity() {
        let a: Vec<&[u8]> = vec![b"user", b"bob", b"logged", b"in", b"from", b"via", b"ssh"];
        let b: Vec<&[u8]> = vec![b"user", b"alice", b"logged", b"in", b"from", b"via", b"winbox"];
        assert!((similarity(&a, &b) - 10.0 / 14.0).abs() < 1e-9);
        let c: Vec<&[u8]> = vec![b"sshd", b"Failed", b"password", b"for", b"root", b"from", b"port", b"ssh2"];
        let d: Vec<&[u8]> = vec![b"sshd", b"Accepted", b"publickey", b"for", b"bob", b"from", b"port", b"ssh2"];
        assert!((similarity(&c, &d) - 10.0 / 16.0).abs() < 1e-9);
        assert_eq!(lcs(3, 0, |_, _| true), vec![]);
        assert_eq!(similarity(&[], &[]), 1.0);
    }

    #[test]
    fn alignment_prefers_one_gap_and_the_earliest_match() {
        let a = ["a", "b", "c", "d"];
        let b = ["a", "c", "d"];
        assert_eq!(align(4, 3, |i, j| u16::from(a[i] == b[j]), |_, _| true), vec![(0, 0), (2, 1), (3, 2)]);
        assert_eq!(align(3, 0, |_, _| 1, |_, _| true), vec![]);
        assert_eq!(align(0, 3, |_, _| 1, |_, _| true), vec![]);
        // x y , N ( x y ) , len   against   x y , len : the pair aligns with the first pair,
        // leaving the NAT block as one gap, not two
        let p = ["x", "y", ",", "N", "(", "x", "y", ")", ",", "len"];
        let q = ["x", "y", ",", "len"];
        let pairs = align(p.len(), q.len(), |i, j| if p[i] == q[j] { if p[i] == "x" || p[i] == "y" { 1 } else { 3 } } else { 0 }, |_, _| true);
        assert_eq!(pairs, vec![(0, 0), (1, 1), (2, 2), (9, 3)]);
        // ip : port , len N   against   ip : port : the member's port takes the pivot's
        // port (earliest), not the trailing N
        let p = ["ip", ":", "port", ",", "len", "N"];
        let q = ["ip", ":", "port"];
        let pairs = align(p.len(), q.len(), |i, j| match (p[i], q[j]) { ("ip", "ip") => 1, (":", ":") => 3, ("port", "port") | ("N", "port") => 1, _ => 0 }, |_, _| true);
        assert_eq!(pairs, vec![(0, 0), (1, 1), (2, 2)]);
        // a keyword outweighs a slot: {user} from   against   from : from matches from
        let p = ["user", "{u}", "from"];
        let q = ["user", "from"];
        let pairs = align(p.len(), q.len(), |i, j| match (p[i], q[j]) { ("user", "user") | ("from", "from") => 3, ("{u}", _) => 1, _ => 0 }, |_, _| true);
        assert_eq!(pairs, vec![(0, 0), (2, 1)]);
        // Accepted _ publickey for   against   Failed _ password for : the space stays
        // aligned, so each word is its own substitution instead of one region
        let p = ["Accepted", " ", "publickey", " ", "for"];
        let q = ["Failed", " ", "password", " ", "for"];
        let pairs = align(p.len(), q.len(), |i, j| if p[i] != q[j] { 0 } else if p[i] == " " { 1 } else { 3 }, |i, j| p[i] != " " && q[j] != " ");
        assert_eq!(pairs, vec![(1, 1), (3, 3), (4, 4)]);
    }
}
