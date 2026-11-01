//! Voiceprint clustering and matching against known people.
//!
//! - [`agglomerate`]: average-linkage agglomerative clustering on cosine
//!   distance with an adaptive stopping threshold, so there is no speaker
//!   limit.
//! - [`two_tier`]: clusters from the longer lines only, the shorter ones
//!   assigned to them afterwards.
//! - [`match_clusters`]: each cluster centroid against every known person's
//!   voiceprints, with the confident / suggest thresholds from settings.

use serde::{Deserialize, Serialize};

use crate::meetings::types::{PersonId, SpeakerModelId};

pub fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

pub fn norm(a: &[f32]) -> f32 {
    dot(a, a).sqrt()
}

/// Cosine similarity in [-1, 1]; 0 for a zero or mismatched vector.
pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let d = norm(a) * norm(b);
    if d <= f32::EPSILON {
        0.0
    } else {
        (dot(a, b) / d).clamp(-1.0, 1.0)
    }
}

/// `v` scaled to unit length (unchanged when zero).
pub fn normalized(v: &[f32]) -> Vec<f32> {
    let n = norm(v);
    if n <= f32::EPSILON {
        v.to_vec()
    } else {
        v.iter().map(|x| x / n).collect()
    }
}

/// Unit-length mean of unit-length copies of `vs` (`None` when empty).
pub fn centroid<'a>(vs: impl IntoIterator<Item = &'a [f32]>) -> Option<Vec<f32>> {
    let mut sum: Option<Vec<f32>> = None;
    for v in vs {
        let v = normalized(v);
        match &mut sum {
            None => sum = Some(v),
            Some(s) if s.len() == v.len() => s.iter_mut().zip(&v).for_each(|(a, b)| *a += b),
            Some(_) => {}
        }
    }
    sum.map(|s| normalized(&s))
}

#[derive(Debug, Clone, Copy)]
pub struct ClusterParams {
    /// Clusters closer than this (cosine distance) are always merged.
    pub min_threshold: f32,
    /// Clusters farther apart than this are never merged.
    pub max_threshold: f32,
    /// Cut used when the merge heights show no clear gap.
    pub default_threshold: f32,
    /// A jump between consecutive merge heights at least this large, ending
    /// above `default_threshold` and starting at most at `max_threshold`, is
    /// taken as the boundary between "same speaker" and "different speakers".
    pub min_gap: f32,
}

impl Default for ClusterParams {
    fn default() -> Self {
        // Two people merged into one is worse than one person split in two
        // (naming the second "Speaker" fixes that, nothing fixes a merge):
        // call audio squeezes voices together, so the cut stays low.
        Self {
            min_threshold: 0.35,
            max_threshold: 0.65,
            default_threshold: 0.5,
            min_gap: 0.1,
        }
    }
}

impl ClusterParams {
    /// The parameters for voiceprints of `model`. All models share the
    /// default values until the evaluation harness measures them apart.
    pub fn for_model(_model: SpeakerModelId) -> Self {
        Self::default()
    }
}

/// A member of a cluster this unlike the rest of it (cosine similarity to
/// the centroid of the other members) is taken out: it is someone else, or
/// two voices in one segment. Only for clusters of at least
/// [`OUTLIER_MIN_CLUSTER`].
pub const OUTLIER_MIN_SIMILARITY: f32 = 0.4;
pub const OUTLIER_MIN_CLUSTER: usize = 3;

/// `labels` (one per vector of `vectors`) with the members that don't sound
/// like the rest of their cluster unlabelled (see [`OUTLIER_MIN_SIMILARITY`]).
pub fn reject_outliers(vectors: &[Vec<f32>], labels: &[usize]) -> Vec<Option<usize>> {
    let units: Vec<Vec<f32>> = vectors.iter().map(|v| normalized(v)).collect();
    let mut sums: std::collections::HashMap<usize, (Vec<f32>, usize)> = Default::default();
    for (v, &l) in units.iter().zip(labels) {
        let entry = sums.entry(l).or_insert_with(|| (vec![0.0; v.len()], 0));
        if entry.0.len() == v.len() {
            entry.0.iter_mut().zip(v).for_each(|(a, b)| *a += b);
            entry.1 += 1;
        }
    }
    units
        .iter()
        .zip(labels)
        .map(|(v, &l)| {
            let (sum, n) = &sums[&l];
            if *n < OUTLIER_MIN_CLUSTER {
                return Some(l);
            }
            // The centroid of the other members.
            let rest: Vec<f32> = sum.iter().zip(v).map(|(s, x)| s - x).collect();
            (cosine(v, &rest) >= OUTLIER_MIN_SIMILARITY).then_some(l)
        })
        .collect()
}

/// One merge of the dendrogram: clusters `a` and `b` (indices of their first
/// members) joined at cosine distance `height`.
#[derive(Debug, Clone, Copy)]
struct Merge {
    a: usize,
    b: usize,
    height: f32,
}

/// Average-linkage dendrogram over cosine distances. Average linkage is
/// monotone, so the merges come out in non-decreasing height. Keeps each
/// row's nearest neighbour cached, which makes it roughly O(n²) in practice.
fn dendrogram(embeddings: &[Vec<f32>]) -> Vec<Merge> {
    let n = embeddings.len();
    if n < 2 {
        return Vec::new();
    }
    let units: Vec<Vec<f32>> = embeddings.iter().map(|e| normalized(e)).collect();
    let mut dist = vec![0f32; n * n];
    for i in 0..n {
        for j in (i + 1)..n {
            let d = 1.0 - cosine(&units[i], &units[j]);
            dist[i * n + j] = d;
            dist[j * n + i] = d;
        }
    }
    let mut size = vec![1usize; n];
    let mut alive = vec![true; n];
    let nearest = |dist: &[f32], alive: &[bool], i: usize| -> (usize, f32) {
        let mut best = (usize::MAX, f32::INFINITY);
        for j in 0..n {
            if j != i && alive[j] && dist[i * n + j] < best.1 {
                best = (j, dist[i * n + j]);
            }
        }
        best
    };
    let mut nn: Vec<(usize, f32)> = (0..n).map(|i| nearest(&dist, &alive, i)).collect();
    let mut merges = Vec::with_capacity(n - 1);
    for _ in 0..n - 1 {
        let (i, (j, height)) = (0..n)
            .filter(|&i| alive[i])
            .map(|i| (i, nn[i]))
            .min_by(|x, y| x.1 .1.total_cmp(&y.1 .1))
            .expect("at least two clusters alive");
        let (keep, gone) = (i.min(j), i.max(j));
        merges.push(Merge {
            a: keep,
            b: gone,
            height,
        });
        // Lance-Williams update for average linkage.
        let (sk, sg) = (size[keep] as f32, size[gone] as f32);
        alive[gone] = false;
        for k in 0..n {
            if alive[k] && k != keep {
                let d = (sk * dist[keep * n + k] + sg * dist[gone * n + k]) / (sk + sg);
                dist[keep * n + k] = d;
                dist[k * n + keep] = d;
            }
        }
        size[keep] += size[gone];
        for k in 0..n {
            if !alive[k] {
                continue;
            }
            let (to, d) = nn[k];
            if k == keep || to == keep || to == gone {
                nn[k] = nearest(&dist, &alive, k);
            } else if dist[k * n + keep] < d {
                nn[k] = (keep, dist[k * n + keep]);
            }
        }
    }
    merges
}

/// The adaptive cut: the midpoint of the largest jump between consecutive
/// merge heights that ends above the default cut, or the default when there
/// is no such jump. A jump ending at or below the default sits among merges
/// of one voice: cutting there would split that voice.
fn choose_cut(heights: &[f32], p: &ClusterParams) -> f32 {
    let mut best: Option<(f32, f32)> = None; // (gap, cut)
    for w in heights.windows(2) {
        let (lo, hi) = (w[0], w[1]);
        if hi <= p.default_threshold || lo > p.max_threshold {
            continue;
        }
        let gap = hi - lo;
        if gap >= p.min_gap && best.is_none_or(|(g, _)| gap > g) {
            let cut = ((lo + hi) / 2.0).clamp(p.min_threshold, p.max_threshold);
            best = Some((gap, cut));
        }
    }
    best.map(|(_, cut)| cut).unwrap_or(p.default_threshold)
}

/// Cluster `embeddings` into speakers. Returns one label per embedding,
/// numbered from 0 in order of first appearance, and the number of clusters.
pub fn agglomerate(embeddings: &[Vec<f32>], params: &ClusterParams) -> (Vec<usize>, usize) {
    let n = embeddings.len();
    if n == 0 {
        return (Vec::new(), 0);
    }
    let merges = dendrogram(embeddings);
    let heights: Vec<f32> = merges.iter().map(|m| m.height).collect();
    let cut = choose_cut(&heights, params);

    let mut parent: Vec<usize> = (0..n).collect();
    fn find(parent: &mut [usize], mut x: usize) -> usize {
        while parent[x] != x {
            parent[x] = parent[parent[x]];
            x = parent[x];
        }
        x
    }
    for m in merges.iter().take_while(|m| m.height <= cut) {
        let (ra, rb) = (find(&mut parent, m.a), find(&mut parent, m.b));
        if ra != rb {
            parent[rb] = ra;
        }
    }
    let mut label_of_root = std::collections::HashMap::new();
    let mut labels = Vec::with_capacity(n);
    for i in 0..n {
        let root = find(&mut parent, i);
        let next = label_of_root.len();
        labels.push(*label_of_root.entry(root).or_insert(next));
    }
    (labels, label_of_root.len())
}

/// Only lines at least this long form clusters: a shorter voiceprint can dip
/// far enough inside one voice (see `turns::TurnParams`) to look like another.
pub const ANCHOR_MIN_MS: i64 = 3_000;
/// A cluster with less speech than this is too little to call a speaker: its
/// lines go back to being assigned.
pub const MIN_CLUSTER_MS: i64 = 6_000;
/// An assigned line must be at least this similar to its cluster's centroid...
pub const ASSIGN_MIN_SIMILARITY: f32 = 0.35;
/// ...and this far ahead of the next cluster's.
pub const ASSIGN_MIN_LEAD: f32 = 0.1;

/// What [`two_tier`] decides about one line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    /// In this cluster (dense, from 0).
    Cluster(usize),
    /// Unlike the rest of its cluster (see [`reject_outliers`]).
    Outlier,
    /// Not clearly anyone's: left to [`attach_by_time`].
    Unplaced,
}

/// Every line clustered as an equal, without [`ANCHOR_MIN_MS`].
fn cluster_all(vectors: &[Vec<f32>], params: &ClusterParams) -> (Vec<Tier>, usize) {
    let (labels, k) = agglomerate(vectors, params);
    let tiers = reject_outliers(vectors, &labels)
        .into_iter()
        .map(|l| l.map_or(Tier::Outlier, Tier::Cluster))
        .collect();
    (tiers, k)
}

/// Unit-length mean of unit-length copies of the vectors, each weighted by
/// its duration (`None` when empty).
fn weighted_centroid<'a>(members: impl IntoIterator<Item = (&'a [f32], i64)>) -> Option<Vec<f32>> {
    let mut sum: Option<Vec<f32>> = None;
    for (v, duration_ms) in members {
        let weight = duration_ms.max(0) as f32;
        let v = normalized(v);
        let s = sum.get_or_insert_with(|| vec![0.0; v.len()]);
        if s.len() == v.len() {
            s.iter_mut().zip(&v).for_each(|(a, b)| *a += weight * b);
        }
    }
    sum.map(|s| normalized(&s))
}

/// Cluster lines (`vectors[i]` lasted `durations_ms[i]`) in two tiers. Lines
/// of at least [`ANCHOR_MIN_MS`] form the clusters; those with under
/// [`MIN_CLUSTER_MS`] of speech dissolve. Every other line goes to the
/// cluster whose duration-weighted centroid is clearly nearest (see
/// [`ASSIGN_MIN_SIMILARITY`], [`ASSIGN_MIN_LEAD`]), else stays
/// [`Tier::Unplaced`]. With fewer than two anchors, or nothing left after
/// dissolving, every line is clustered as an equal instead. Returns one
/// [`Tier`] per line and the number of clusters.
pub fn two_tier(
    vectors: &[Vec<f32>],
    durations_ms: &[i64],
    params: &ClusterParams,
) -> (Vec<Tier>, usize) {
    debug_assert_eq!(vectors.len(), durations_ms.len());
    let anchors: Vec<usize> = (0..vectors.len())
        .filter(|&i| durations_ms[i] >= ANCHOR_MIN_MS)
        .collect();
    if anchors.len() < 2 {
        return cluster_all(vectors, params);
    }
    let anchor_vectors: Vec<Vec<f32>> = anchors.iter().map(|&i| vectors[i].clone()).collect();
    let (labels, _) = agglomerate(&anchor_vectors, params);
    let kept = reject_outliers(&anchor_vectors, &labels);

    let mut speech_ms: std::collections::HashMap<usize, i64> = Default::default();
    for (&i, l) in anchors.iter().zip(&kept) {
        if let Some(l) = l {
            *speech_ms.entry(*l).or_default() += durations_ms[i];
        }
    }
    // Lasting clusters renumbered from 0, in order of first appearance.
    let mut dense: std::collections::HashMap<usize, usize> = Default::default();
    for l in kept.iter().flatten() {
        if speech_ms[l] >= MIN_CLUSTER_MS {
            let next = dense.len();
            dense.entry(*l).or_insert(next);
        }
    }
    if dense.is_empty() {
        return cluster_all(vectors, params);
    }
    let k = dense.len();

    let mut tiers = vec![Tier::Unplaced; vectors.len()];
    for (&i, l) in anchors.iter().zip(&kept) {
        tiers[i] = match l {
            None => Tier::Outlier,
            Some(l) => dense.get(l).map_or(Tier::Unplaced, |&c| Tier::Cluster(c)),
        };
    }
    let centroids: Vec<Vec<f32>> = (0..k)
        .filter_map(|c| {
            weighted_centroid(
                (0..vectors.len())
                    .filter(|&i| tiers[i] == Tier::Cluster(c))
                    .map(|i| (vectors[i].as_slice(), durations_ms[i])),
            )
        })
        .collect();
    let nearest = |v: &[f32]| -> Option<usize> {
        let mut best: Option<(usize, f32)> = None;
        let mut next = f32::NEG_INFINITY;
        for (c, centroid) in centroids.iter().enumerate() {
            let s = cosine(v, centroid);
            match best {
                Some((_, b)) if s <= b => next = next.max(s),
                _ => {
                    next = best.map_or(next, |(_, b)| b);
                    best = Some((c, s));
                }
            }
        }
        let (c, s) = best?;
        (s >= ASSIGN_MIN_SIMILARITY && s - next >= ASSIGN_MIN_LEAD).then_some(c)
    };
    for (v, tier) in vectors.iter().zip(tiers.iter_mut()) {
        if *tier == Tier::Unplaced {
            if let Some(c) = nearest(v) {
                *tier = Tier::Cluster(c);
            }
        }
    }
    (tiers, k)
}

/// Speech this close to its neighbours counts as part of the same turn.
pub const ATTACH_MAX_GAP_MS: i64 = 2_000;

/// Give an unlabelled item (speech too short for a voiceprint) a label only
/// when that is clearly the speaker: the nearest labelled items before and
/// after it agree, and each is within [`ATTACH_MAX_GAP_MS`] (overlapping
/// counts as next to it; a side with nothing labelled doesn't object).
/// Anything else stays unlabelled, an unknown speaker for the user to name,
/// rather than a guess. `spans` are `(t0_ms, t1_ms)`.
pub fn attach_by_time(spans: &[(i64, i64)], labels: &[Option<usize>]) -> Vec<Option<usize>> {
    let labelled: Vec<(usize, usize)> = labels
        .iter()
        .enumerate()
        .filter_map(|(i, l)| l.map(|l| (i, l)))
        .collect();
    labels
        .iter()
        .enumerate()
        .map(|(i, l)| {
            if l.is_some() {
                return *l;
            }
            let (t0, t1) = spans[i];
            // Nearest labelled item ending before / starting after, by gap
            // (overlaps are gap 0, on both sides).
            let side = |before: bool| {
                labelled
                    .iter()
                    .filter_map(|&(j, l)| {
                        let (u0, u1) = spans[j];
                        let gap = if before { t0 - u1 } else { u0 - t1 };
                        let overlaps = u0 < t1 && t0 < u1;
                        let on_side = if before { u0 <= t0 } else { u0 >= t0 };
                        (on_side || overlaps).then_some((gap.max(0), l))
                    })
                    .min_by_key(|(gap, _)| *gap)
            };
            let near = |before: bool| {
                side(before)
                    .filter(|(gap, _)| *gap <= ATTACH_MAX_GAP_MS)
                    .map(|(_, l)| l)
            };
            match (near(true), near(false)) {
                (Some(a), Some(b)) => (a == b).then_some(a),
                (Some(l), None) | (None, Some(l)) => Some(l),
                (None, None) => None,
            }
        })
        .collect()
}

/// A known person's stored voiceprints.
#[derive(Debug, Clone)]
pub struct KnownVoice {
    pub person_id: PersonId,
    pub prints: Vec<Vec<f32>>,
}

/// How strongly `centroid` matches a person: the mean of the best (up to 3)
/// cosine similarities against their prints. Between "max" (one lucky print
/// decides) and "mean" (old or noisy prints drag it down).
pub fn person_similarity(centroid: &[f32], prints: &[Vec<f32>]) -> Option<f32> {
    top_mean(prints.iter().map(|p| cosine(centroid, p)).collect())
}

/// Mean of the best (up to 3) of `sims` (`None` when empty).
fn top_mean(mut sims: Vec<f32>) -> Option<f32> {
    if sims.is_empty() {
        return None;
    }
    sims.sort_by(|a, b| b.total_cmp(a));
    let top = &sims[..sims.len().min(3)];
    Some(top.iter().sum::<f32>() / top.len() as f32)
}

/// A stored print this close to a line's voiceprint was taken from that
/// line (naming or tagging stores them), so it doesn't vouch for it.
pub const SAME_PRINT_SIMILARITY: f32 = 0.999;

/// How far a line's best match must lead the next person's.
pub const LINE_MATCH_MARGIN: f32 = 0.1;

/// The known person one line clearly sounds like: at least `threshold`
/// similar (see [`person_similarity`]), and [`LINE_MATCH_MARGIN`] ahead of
/// everyone else. The line's own prints (see [`SAME_PRINT_SIMILARITY`])
/// are left out; `None` when they are all someone has (nothing then says
/// how much the line sounds like them). That is on purpose, not a missed
/// case: such a person may be whom the line is (their print is it), so no
/// one else can be said to clearly lead them. Skipping them instead would
/// let a line tagged by hand be overruled by another person's prints.
pub fn match_line(voice: &[f32], people: &[KnownVoice], threshold: f32) -> Option<(PersonId, f32)> {
    let mut ranked: Vec<(PersonId, f32)> = Vec::with_capacity(people.len());
    for p in people.iter().filter(|p| !p.prints.is_empty()) {
        let sims = p
            .prints
            .iter()
            .map(|q| cosine(voice, q))
            .filter(|s| *s < SAME_PRINT_SIMILARITY)
            .collect();
        ranked.push((p.person_id, top_mean(sims)?));
    }
    ranked.sort_by(|a, b| b.1.total_cmp(&a.1));
    let (person, best) = *ranked.first()?;
    let next = ranked.get(1).map_or(f32::NEG_INFINITY, |r| r.1);
    (best >= threshold && best - next >= LINE_MATCH_MARGIN).then_some((person, best))
}

/// People ranked by voice similarity to `centroid`, best first.
pub fn rank_people(centroid: &[f32], people: &[KnownVoice]) -> Vec<(PersonId, f32)> {
    let mut ranked: Vec<(PersonId, f32)> = people
        .iter()
        .filter_map(|p| person_similarity(centroid, &p.prints).map(|s| (p.person_id, s)))
        .collect();
    ranked.sort_by(|a, b| b.1.total_cmp(&a.1));
    ranked
}

/// The automatic match of one cluster.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Default)]
pub struct SpeakerMatch {
    pub person_id: Option<PersonId>,
    pub confidence: Option<f32>,
    /// True at or above the match threshold; false means "Confirm?".
    pub confirmed: bool,
}

/// Match clusters to people. Each person goes to at most one cluster (the
/// best-scoring pairs win). At or above `match_threshold` the match is
/// confirmed; at or above `suggest_threshold` it is assigned unconfirmed;
/// below, the cluster stays unassigned ("Speaker N").
pub fn match_clusters(
    centroids: &[Option<Vec<f32>>],
    people: &[KnownVoice],
    match_threshold: f32,
    suggest_threshold: f32,
) -> Vec<SpeakerMatch> {
    let mut pairs: Vec<(usize, PersonId, f32)> = Vec::new();
    for (c, centroid) in centroids.iter().enumerate() {
        let Some(centroid) = centroid else { continue };
        for p in people {
            if let Some(s) = person_similarity(centroid, &p.prints) {
                if s >= suggest_threshold {
                    pairs.push((c, p.person_id, s));
                }
            }
        }
    }
    pairs.sort_by(|a, b| b.2.total_cmp(&a.2));
    let mut out = vec![SpeakerMatch::default(); centroids.len()];
    let mut taken = std::collections::HashSet::new();
    for (c, person, s) in pairs {
        if out[c].person_id.is_some() || taken.contains(&person) {
            continue;
        }
        taken.insert(person);
        out[c] = SpeakerMatch {
            person_id: Some(person),
            confidence: Some(s),
            confirmed: s >= match_threshold,
        };
    }
    out
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Deterministic pseudo-random generator (no rand dependency in tests).
    pub(crate) struct Lcg(pub u64);
    impl Lcg {
        pub(crate) fn next_f32(&mut self) -> f32 {
            self.0 = self
                .0
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((self.0 >> 33) as f32 / (1u64 << 31) as f32) * 2.0 - 1.0
        }
        pub(crate) fn vector(&mut self, dim: usize) -> Vec<f32> {
            (0..dim).map(|_| self.next_f32()).collect()
        }
    }

    /// `base` plus noise of relative size `noise`.
    pub(crate) fn jitter(rng: &mut Lcg, base: &[f32], noise: f32) -> Vec<f32> {
        let b = normalized(base);
        let n = normalized(&rng.vector(b.len()));
        normalized(
            &b.iter()
                .zip(&n)
                .map(|(x, y)| x + noise * y)
                .collect::<Vec<_>>(),
        )
    }

    #[test]
    fn a_line_matches_the_person_it_clearly_sounds_like() {
        let people = [
            KnownVoice {
                person_id: 1,
                prints: vec![vec![1.0, 0.0], vec![0.95, 0.31]],
            },
            KnownVoice {
                person_id: 2,
                prints: vec![vec![0.0, 1.0]],
            },
        ];
        let line = normalized(&[1.0, 0.1]);
        assert_eq!(match_line(&line, &people, 0.65).map(|m| m.0), Some(1));
        // Not similar enough to anyone.
        assert_eq!(match_line(&line, &people, 0.999), None);
        // Halfway between the two: neither leads by the margin.
        let between = normalized(&[1.0, 1.0]);
        assert_eq!(match_line(&between, &people, 0.5), None);
        // Its own print (the same vector) doesn't count.
        let own = vec![1.0, 0.0];
        let m = match_line(&own, &people, 0.65).unwrap();
        assert_eq!(m.0, 1);
        assert!(m.1 < SAME_PRINT_SIMILARITY);
    }

    #[test]
    fn three_well_separated_speakers_make_three_clusters() {
        let mut rng = Lcg(7);
        let speakers: Vec<Vec<f32>> = (0..3).map(|_| rng.vector(192)).collect();
        // Interleaved turns: A B C A B C ...
        let mut embs = Vec::new();
        let mut truth = Vec::new();
        for i in 0..30 {
            let s = i % 3;
            embs.push(jitter(&mut rng, &speakers[s], 0.5));
            truth.push(s);
        }
        let (labels, k) = agglomerate(&embs, &ClusterParams::default());
        assert_eq!(k, 3, "{labels:?}");
        // Same speaker <=> same label.
        for i in 0..embs.len() {
            for j in 0..embs.len() {
                assert_eq!(truth[i] == truth[j], labels[i] == labels[j]);
            }
        }
        // Labels in order of first appearance.
        assert_eq!(&labels[..3], &[0, 1, 2]);
    }

    #[test]
    fn many_speakers_have_no_limit() {
        let mut rng = Lcg(11);
        let speakers: Vec<Vec<f32>> = (0..9).map(|_| rng.vector(256)).collect();
        let embs: Vec<Vec<f32>> = (0..90)
            .map(|i| jitter(&mut rng, &speakers[i % 9], 0.45))
            .collect();
        let (_, k) = agglomerate(&embs, &ClusterParams::default());
        assert_eq!(k, 9);
    }

    #[test]
    fn one_speaker_stays_one_cluster() {
        let mut rng = Lcg(3);
        let base = rng.vector(128);
        let embs: Vec<Vec<f32>> = (0..12).map(|_| jitter(&mut rng, &base, 0.5)).collect();
        let (labels, k) = agglomerate(&embs, &ClusterParams::default());
        assert_eq!(k, 1, "{labels:?}");
    }

    #[test]
    fn two_segments_of_two_speakers() {
        let mut rng = Lcg(5);
        let embs = vec![rng.vector(64), rng.vector(64)];
        assert_eq!(agglomerate(&embs, &ClusterParams::default()).1, 2);
        assert_eq!(agglomerate(&embs[..1], &ClusterParams::default()).1, 1);
        assert_eq!(agglomerate(&[], &ClusterParams::default()).1, 0);
    }

    #[test]
    fn jumps_among_merges_of_one_voice_do_not_split_it() {
        let p = ClusterParams::default();
        let heights = [0.20, 0.25, 0.38, 0.49];
        assert_eq!(choose_cut(&heights, &p), p.default_threshold);
        // Every merge happens: one cluster.
        assert!(heights.iter().all(|h| *h <= choose_cut(&heights, &p)));
        // A jump ending exactly at the default is still not above it.
        assert_eq!(choose_cut(&[0.3, 0.5], &p), p.default_threshold);
    }

    #[test]
    fn a_jump_straddling_the_default_still_splits() {
        let p = ClusterParams::default();
        let heights = [0.3, 0.45, 0.8];
        let cut = choose_cut(&heights, &p);
        assert!((cut - 0.625).abs() < 1e-6, "{cut}");
        // The last merge doesn't happen: two clusters.
        assert_eq!(heights.iter().filter(|h| **h <= cut).count(), 2);
    }

    #[test]
    fn heights_without_a_jump_give_the_default_cut() {
        let p = ClusterParams::default();
        for heights in [&[][..], &[0.9][..], &[0.6, 0.6, 0.6][..]] {
            assert_eq!(choose_cut(heights, &p), p.default_threshold);
        }
    }

    #[test]
    fn every_model_shares_the_default_parameters_for_now() {
        let (d, m) = (
            ClusterParams::default(),
            ClusterParams::for_model(SpeakerModelId::TitanetLarge),
        );
        assert_eq!(d.default_threshold, m.default_threshold);
        assert_eq!(d.min_threshold, m.min_threshold);
        assert_eq!(d.max_threshold, m.max_threshold);
        assert_eq!(d.min_gap, m.min_gap);
    }

    #[test]
    fn short_segments_join_a_speaker_only_when_both_sides_agree() {
        let spans = [
            (0, 5000),
            (5100, 5800),
            (6000, 12000),
            (12100, 12500),
            (16000, 16400),
            (21000, 26000),
            (26100, 26500),
            (26600, 30000),
        ];
        let labels = [Some(0), None, Some(1), None, None, Some(2), None, Some(2)];
        let got = attach_by_time(&spans, &labels);
        assert_eq!(
            got,
            vec![
                Some(0),
                None,    // between two speakers: unknown, not a guess
                Some(1), // right after speaker 1, nothing after it nearby
                Some(1),
                None, // seconds from anyone: unknown
                Some(2),
                Some(2), // inside one speaker's turn
                Some(2),
            ]
        );
        assert_eq!(attach_by_time(&[(0, 1)], &[None]), vec![None]);
    }

    #[test]
    fn noisy_short_lines_make_no_clusters() {
        let mut rng = Lcg(31);
        let base = rng.vector(128);
        let mut vectors = vec![jitter(&mut rng, &base, 0.3), jitter(&mut rng, &base, 0.3)];
        let mut durations = vec![5_000, 5_000];
        for _ in 0..10 {
            vectors.push(rng.vector(128));
            durations.push(2_000);
        }
        let (tiers, k) = two_tier(&vectors, &durations, &ClusterParams::default());
        assert_eq!(k, 1);
        assert_eq!(&tiers[..2], &[Tier::Cluster(0), Tier::Cluster(0)]);
        assert!(tiers[2..].iter().all(|t| *t == Tier::Unplaced), "{tiers:?}");
    }

    #[test]
    fn a_short_line_joins_the_clearly_nearest_cluster_only() {
        let (a, b) = (vec![1.0, 0.0, 0.0], vec![0.0, 1.0, 0.0]);
        let between = vec![1.0, 1.0, 0.0];
        let vectors = vec![
            a.clone(),
            a.clone(),
            b.clone(),
            b.clone(),
            vec![1.0, 0.2, 0.0],
            between,
        ];
        let durations = [4_000, 4_000, 4_000, 4_000, 2_000, 2_000];
        let (tiers, k) = two_tier(&vectors, &durations, &ClusterParams::default());
        assert_eq!(k, 2);
        assert_eq!(tiers[4], Tier::Cluster(0));
        // As near one as the other: no lead.
        assert_eq!(tiers[5], Tier::Unplaced);
    }

    #[test]
    fn a_tiny_cluster_dissolves_and_labels_stay_dense() {
        let (a, b) = (vec![1.0, 0.0, 0.0], vec![0.0, 1.0, 0.0]);
        // B comes first and has too little speech to be a speaker.
        let vectors = vec![b, a.clone(), a];
        let (tiers, k) = two_tier(&vectors, &[3_500, 4_000, 4_000], &ClusterParams::default());
        assert_eq!(k, 1);
        assert_eq!(
            tiers,
            vec![Tier::Unplaced, Tier::Cluster(0), Tier::Cluster(0)]
        );
    }

    #[test]
    fn long_lines_dominate_the_centroid() {
        let (a, b) = ([1.0f32, 0.0], [0.0f32, 1.0]);
        let c = weighted_centroid([(&a[..], 30_000), (&b[..], 3_000)]).unwrap();
        assert!(cosine(&c, &a) > 0.99, "{c:?}");
        assert!((norm(&c) - 1.0).abs() < 1e-6);
        assert!(weighted_centroid(std::iter::empty::<(&[f32], i64)>()).is_none());
        // Weights of nothing: a zero vector, not NaN.
        let c = weighted_centroid([(&a[..], 0)]).unwrap();
        assert!(c.iter().all(|x| *x == 0.0), "{c:?}");
    }

    #[test]
    fn durations_stay_with_their_vectors() {
        // Which lines are anchors follows the durations, not the order.
        let (a, b) = (vec![1.0, 0.0, 0.0], vec![0.0, 1.0, 0.0]);
        let vectors = vec![b, a.clone(), a];
        let (tiers, k) = two_tier(&vectors, &[1_600, 4_000, 4_000], &ClusterParams::default());
        assert_eq!(k, 1);
        assert_eq!(tiers[0], Tier::Unplaced);
    }

    #[test]
    fn fewer_than_two_anchors_cluster_every_line_as_before() {
        let mut rng = Lcg(37);
        let (a, b) = (rng.vector(64), rng.vector(64));
        let vectors = vec![
            jitter(&mut rng, &a, 0.3),
            jitter(&mut rng, &b, 0.3),
            jitter(&mut rng, &a, 0.3),
            jitter(&mut rng, &b, 0.3),
        ];
        let params = ClusterParams::default();
        let (labels, k) = agglomerate(&vectors, &params);
        let want: Vec<Tier> = labels.iter().map(|l| Tier::Cluster(*l)).collect();
        for durations in [[2_000, 2_000, 2_000, 2_000], [2_000, 9_000, 2_000, 2_000]] {
            assert_eq!(two_tier(&vectors, &durations, &params), (want.clone(), k));
        }
        assert_eq!(two_tier(&[], &[], &params), (Vec::new(), 0));
    }

    #[test]
    fn two_brief_voices_are_not_dissolved_into_nothing() {
        let vectors = vec![vec![1.0, 0.0], vec![0.0, 1.0]];
        let (tiers, k) = two_tier(&vectors, &[4_000, 4_000], &ClusterParams::default());
        assert_eq!(k, 2);
        assert_eq!(tiers, vec![Tier::Cluster(0), Tier::Cluster(1)]);
    }

    #[test]
    fn a_voice_unlike_the_rest_of_its_cluster_is_taken_out() {
        let a = |x: f32| vec![1.0, x, 0.0];
        let other = vec![0.0, 0.2, 1.0];
        let vectors = vec![a(0.1), a(0.0), a(0.2), other.clone(), a(-0.1)];
        let got = reject_outliers(&vectors, &[0, 0, 0, 0, 0]);
        assert_eq!(got, vec![Some(0), Some(0), Some(0), None, Some(0)]);
        // Too small a cluster to judge.
        let got = reject_outliers(&[a(0.0), other], &[0, 0]);
        assert_eq!(got, vec![Some(0), Some(0)]);
    }

    #[test]
    fn thresholds_decide_confirmed_unconfirmed_and_unassigned() {
        let mut rng = Lcg(21);
        let alice = rng.vector(64);
        let bob = rng.vector(64);
        let carol = rng.vector(64);
        let people = vec![
            KnownVoice {
                person_id: 1,
                prints: vec![jitter(&mut rng, &alice, 0.2), jitter(&mut rng, &alice, 0.2)],
            },
            KnownVoice {
                person_id: 2,
                prints: vec![jitter(&mut rng, &bob, 0.2)],
            },
        ];
        // Alice close; "bob-ish" halfway between Bob and Carol; Carol unknown.
        let bobish: Vec<f32> = normalized(
            &normalized(&bob)
                .iter()
                .zip(normalized(&carol))
                .map(|(a, b)| a + 1.5 * b)
                .collect::<Vec<_>>(),
        );
        let centroids = vec![
            Some(jitter(&mut rng, &alice, 0.2)),
            Some(bobish.clone()),
            Some(normalized(&carol)),
            None,
        ];
        let bob_sim = person_similarity(&bobish, &people[1].prints).unwrap();
        assert!((0.45..0.65).contains(&bob_sim), "{bob_sim}");

        let m = match_clusters(&centroids, &people, 0.65, 0.45);
        assert_eq!(m[0].person_id, Some(1));
        assert!(m[0].confirmed);
        assert_eq!(m[1].person_id, Some(2));
        assert!(!m[1].confirmed, "between thresholds: Confirm?");
        assert_eq!(m[2], SpeakerMatch::default());
        assert_eq!(m[3], SpeakerMatch::default());
    }

    #[test]
    fn a_person_matches_at_most_one_cluster() {
        let mut rng = Lcg(9);
        let alice = rng.vector(64);
        let people = vec![KnownVoice {
            person_id: 1,
            prints: vec![normalized(&alice)],
        }];
        let near = jitter(&mut rng, &alice, 0.1);
        let nearer = jitter(&mut rng, &alice, 0.02);
        let m = match_clusters(&[Some(near), Some(nearer)], &people, 0.65, 0.45);
        assert_eq!(m[0].person_id, None);
        assert_eq!(m[1].person_id, Some(1));
    }

    #[test]
    fn ranks_people_by_similarity() {
        let mut rng = Lcg(13);
        let a = rng.vector(32);
        let b = rng.vector(32);
        let people = vec![
            KnownVoice {
                person_id: 1,
                prints: vec![b.clone()],
            },
            KnownVoice {
                person_id: 2,
                prints: vec![a.clone()],
            },
            KnownVoice {
                person_id: 3,
                prints: vec![],
            },
        ];
        let ranked = rank_people(&jitter(&mut rng, &a, 0.1), &people);
        assert_eq!(ranked.len(), 2);
        assert_eq!(ranked[0].0, 2);
    }

    #[test]
    fn cosine_and_centroid_basics() {
        assert!((cosine(&[1.0, 0.0], &[2.0, 0.0]) - 1.0).abs() < 1e-6);
        assert_eq!(cosine(&[1.0, 0.0], &[0.0, 0.0]), 0.0);
        assert_eq!(cosine(&[1.0], &[1.0, 2.0]), 0.0);
        let c = centroid([&[1.0f32, 0.0][..], &[0.0, 3.0][..]]).unwrap();
        assert!((c[0] - c[1]).abs() < 1e-6 && (norm(&c) - 1.0).abs() < 1e-6);
        assert!(centroid(std::iter::empty::<&[f32]>()).is_none());
    }
}
