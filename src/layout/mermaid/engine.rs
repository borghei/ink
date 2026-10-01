//! Layered (Sugiyama-style) graph layout on a character grid.
//!
//! 1. Self-loops are set aside; cycles are broken by reversing DFS back
//!    edges.
//! 2. Ranks by longest path (sources pulled down next to their targets).
//!    When any edge has a label every edge spans two ranks, and the label
//!    becomes a node of its own in the middle rank, so the layout makes room
//!    for it like for any other node.
//! 3. Edges spanning several ranks get one dummy node per rank.
//! 4. Crossings are reduced with barycenter sweeps (members of a subgraph
//!    are kept contiguous in every rank), keeping the best order seen.
//! 5. Cross-axis coordinates: repeated weighted isotonic regression
//!    (pool-adjacent-violators) towards each node's neighbours, which centres
//!    parents over children and straightens long edges.
//! 6. Edges between adjacent ranks are routed orthogonally through the gap
//!    between them, each source's fan-out sharing one horizontal track.
//!
//! The engine works in (main, cross) coordinates — main runs along the
//! ranks — and maps them to (row, col) at the end, so `LR` is the same code
//! as `TD`. `BT` and `RL` flip the ranks.

use super::canvas::{Canvas, Class, DOWN, LEFT, RIGHT, UP};
use super::graph::{Dir, Graph, LineStyle, Marker, Shape};
use super::text;

/// Diagrams with more nodes than this are listed, not laid out.
pub const MAX_NODES: usize = 200;
/// Diagrams with more edges than this are listed, not laid out.
pub const MAX_EDGES: usize = 400;
/// Cap on layout nodes including the dummies long edges create.
const MAX_LAYOUT_NODES: usize = 4000;
/// Cap on canvas cells.
const MAX_CELLS: usize = 4_000_000;

/// Knobs one layout attempt runs with.
#[derive(Debug, Clone, Copy)]
pub struct Params {
    /// Wrap node labels to this many columns.
    pub wrap: usize,
    /// Space between neighbouring nodes in a rank.
    pub sep: usize,
    pub dir: Dir,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Fail {
    /// Over the node, edge or cell budget.
    TooBig,
    /// Laid out wider than the space available (the width it needed).
    TooWide(usize),
}

/// A drawn diagram.
pub struct Drawn {
    pub rows: Vec<Vec<(String, Class)>>,
    /// Subgraph frames could not be drawn without overlapping other nodes;
    /// membership is listed in `notes` instead.
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Real(usize),
    Dummy,
    Label,
}

struct LNode {
    kind: Kind,
    /// Edge a dummy or label node belongs to.
    edge: usize,
    rank: usize,
    cluster_path: Vec<usize>,
    cross: usize,
    main: usize,
    ups: Vec<usize>,
    downs: Vec<usize>,
}

struct Seg {
    up: usize,
    down: usize,
    edge: usize,
    /// Port offsets (cross axis, relative to the node's cross start) at the
    /// up and down ends.
    port_up: usize,
    port_down: usize,
}

struct EdgeInfo {
    /// The edge's `from` is drawn at the bottom (it points against the
    /// ranks: a broken cycle, or a `BT`/`RL` diagram).
    rev: bool,
    /// Reversed to break a cycle.
    cyc: bool,
    label: Vec<String>,
}

/// Geometry of a real node's box.
struct NodeBox {
    w: usize,
    h: usize,
    /// Title lines.
    lines: Vec<String>,
    /// Further compartments (wrapped rows).
    sections: Vec<Vec<String>>,
    /// Self-loop label, when the node has a self-loop.
    loop_label: Option<String>,
}

pub fn layout(g: &Graph, p: &Params, avail: usize) -> Result<Drawn, Fail> {
    if g.nodes.len() > MAX_NODES || g.edges.len() > MAX_EDGES || g.nodes.is_empty() {
        return Err(Fail::TooBig);
    }
    let horiz = p.dir.horizontal();
    let n = g.nodes.len();

    // ── Boxes and self-loops ──
    let mut loops: Vec<Vec<String>> = vec![Vec::new(); n];
    let mut edges: Vec<usize> = Vec::new();
    for (i, e) in g.edges.iter().enumerate() {
        if e.from == e.to {
            loops[e.from].push(e.label.replace('\n', " "));
        } else {
            edges.push(i);
        }
    }
    let mut boxes: Vec<NodeBox> = g
        .nodes
        .iter()
        .enumerate()
        .map(|(i, node)| {
            node_box(
                node.shape,
                &node.label,
                &node.sections,
                p.wrap,
                horiz,
                &loops[i],
            )
        })
        .collect();

    // ── Cycle breaking ──
    let mut out_adj: Vec<Vec<usize>> = vec![Vec::new(); n];
    for &ei in &edges {
        out_adj[g.edges[ei].from].push(ei);
    }
    let mut cyc = vec![false; g.edges.len()];
    {
        let mut state = vec![0u8; n];
        for s in 0..n {
            if state[s] != 0 {
                continue;
            }
            let mut stack: Vec<(usize, usize)> = vec![(s, 0)];
            state[s] = 1;
            while let Some(&mut (v, ref mut k)) = stack.last_mut() {
                if *k < out_adj[v].len() {
                    let ei = out_adj[v][*k];
                    *k += 1;
                    let t = g.edges[ei].to;
                    match state[t] {
                        0 => {
                            state[t] = 1;
                            stack.push((t, 0));
                        }
                        1 => cyc[ei] = true,
                        _ => {}
                    }
                } else {
                    state[v] = 2;
                    stack.pop();
                }
            }
        }
    }

    // ── Ranks ──
    let labelled = |ei: usize| {
        let e = &g.edges[ei];
        !e.label.is_empty() || e.cards.is_some()
    };
    let minlen = if edges.iter().any(|&ei| labelled(ei)) {
        2
    } else {
        1
    };
    let dag: Vec<(usize, usize)> = edges
        .iter()
        .map(|&ei| {
            let e = &g.edges[ei];
            if cyc[ei] {
                (e.to, e.from)
            } else {
                (e.from, e.to)
            }
        })
        .collect();
    let mut rank = vec![0usize; n];
    {
        let mut indeg = vec![0usize; n];
        let mut succ: Vec<Vec<usize>> = vec![Vec::new(); n];
        for &(a, b) in &dag {
            indeg[b] += 1;
            succ[a].push(b);
        }
        let mut queue: std::collections::VecDeque<usize> =
            (0..n).filter(|&v| indeg[v] == 0).collect();
        let mut order = Vec::with_capacity(n);
        let mut deg = indeg.clone();
        while let Some(v) = queue.pop_front() {
            order.push(v);
            for &w in &succ[v] {
                rank[w] = rank[w].max(rank[v] + minlen);
                deg[w] -= 1;
                if deg[w] == 0 {
                    queue.push_back(w);
                }
            }
        }
        // Pull sources down next to their nearest target.
        for &v in &order {
            if indeg[v] == 0 && !succ[v].is_empty() {
                let lowest = succ[v].iter().map(|&w| rank[w]).min().unwrap_or(minlen);
                rank[v] = lowest.saturating_sub(minlen);
            }
        }
    }
    let max_rank = rank.iter().copied().max().unwrap_or(0);
    if p.dir.reversed() {
        for r in &mut rank {
            *r = max_rank - *r;
        }
    }
    let nranks = max_rank + 1;

    // ── Layout nodes, dummies, labels ──
    let node_path: Vec<Vec<usize>> = g
        .nodes
        .iter()
        .map(|nd| g.cluster_path(nd.cluster))
        .collect();
    let mut lnodes: Vec<LNode> = (0..n)
        .map(|i| LNode {
            kind: Kind::Real(i),
            edge: usize::MAX,
            rank: rank[i],
            cluster_path: node_path[i].clone(),
            cross: 0,
            main: 0,
            ups: Vec::new(),
            downs: Vec::new(),
        })
        .collect();
    let mut segs: Vec<Seg> = Vec::new();
    let mut einfo: Vec<Option<EdgeInfo>> = (0..g.edges.len()).map(|_| None).collect();
    for &ei in &edges {
        let e = &g.edges[ei];
        let (top, bottom) = if rank[e.from] < rank[e.to] {
            (e.from, e.to)
        } else {
            (e.to, e.from)
        };
        let rev = top != e.from;
        // Label text, with cardinalities ordered top to bottom.
        let mut parts: Vec<String> = Vec::new();
        let (c_top, c_bottom) = match &e.cards {
            Some((a, b)) if rev => (b.clone(), a.clone()),
            Some((a, b)) => (a.clone(), b.clone()),
            None => (String::new(), String::new()),
        };
        if horiz {
            let joined = [c_top.as_str(), e.label.as_str(), c_bottom.as_str()]
                .iter()
                .filter(|s| !s.is_empty())
                .map(|s| s.replace('\n', " "))
                .collect::<Vec<_>>()
                .join(" ");
            if !joined.is_empty() {
                parts.extend(text::wrap(&joined, p.wrap));
            }
        } else {
            for s in [&c_top, &e.label, &c_bottom] {
                if !s.is_empty() {
                    parts.extend(text::wrap(s, p.wrap));
                }
            }
        }
        // Common subgraph of both ends.
        let pa = &node_path[e.from];
        let pb = &node_path[e.to];
        let common: Vec<usize> = pa
            .iter()
            .zip(pb.iter())
            .take_while(|(a, b)| a == b)
            .map(|(a, _)| *a)
            .collect();
        let (r0, r1) = (rank[top], rank[bottom]);
        let label_rank = if parts.is_empty() {
            usize::MAX
        } else {
            let mid = r0 + (r1 - r0) / 2;
            if (mid - r0) % 2 == 0 && mid > r0 + 1 {
                mid - 1
            } else {
                mid
            }
        };
        let mut prev = top;
        for r in r0 + 1..=r1 {
            let cur = if r == r1 {
                bottom
            } else {
                if lnodes.len() >= MAX_LAYOUT_NODES {
                    return Err(Fail::TooBig);
                }
                lnodes.push(LNode {
                    kind: if r == label_rank {
                        Kind::Label
                    } else {
                        Kind::Dummy
                    },
                    edge: ei,
                    rank: r,
                    cluster_path: common.clone(),
                    cross: 0,
                    main: 0,
                    ups: Vec::new(),
                    downs: Vec::new(),
                });
                lnodes.len() - 1
            };
            let si = segs.len();
            segs.push(Seg {
                up: prev,
                down: cur,
                edge: ei,
                port_up: 0,
                port_down: 0,
            });
            lnodes[prev].downs.push(si);
            lnodes[cur].ups.push(si);
            prev = cur;
        }
        einfo[ei] = Some(EdgeInfo {
            rev,
            cyc: cyc[ei],
            label: parts,
        });
    }
    let info = |ei: usize| einfo[ei].as_ref().expect("edge info for a laid-out edge");

    // ── Crossing reduction ──
    let mut ranks: Vec<Vec<usize>> = vec![Vec::new(); nranks];
    for (i, ln) in lnodes.iter().enumerate() {
        ranks[ln.rank].push(i);
    }
    let mut pos = vec![0usize; lnodes.len()];
    let set_pos = |ranks: &Vec<Vec<usize>>, pos: &mut Vec<usize>| {
        for rk in ranks {
            for (i, &l) in rk.iter().enumerate() {
                pos[l] = i;
            }
        }
    };
    // Group subgraph members together from the start.
    for rk in &mut ranks {
        let mut items: Vec<(usize, f64)> =
            rk.iter().enumerate().map(|(i, &l)| (l, i as f64)).collect();
        group_sort(&mut items, &lnodes, 0);
        *rk = items.into_iter().map(|(l, _)| l).collect();
    }
    set_pos(&ranks, &mut pos);
    let mut best = ranks.clone();
    let mut best_cross = count_crossings(&ranks, &lnodes, &segs, &pos);
    let budget = 2_000_000usize;
    let iters = (budget / (segs.len() + lnodes.len() + 1)).clamp(1, 12);
    let mut stale = 0;
    for it in 0..iters {
        if best_cross == 0 {
            break;
        }
        let down = it % 2 == 0;
        let order: Vec<usize> = if down {
            (1..nranks).collect()
        } else {
            (0..nranks.saturating_sub(1)).rev().collect()
        };
        for r in order {
            let mut items: Vec<(usize, f64)> = ranks[r]
                .iter()
                .map(|&l| {
                    let nb: Vec<usize> = if down {
                        lnodes[l].ups.iter().map(|&s| pos[segs[s].up]).collect()
                    } else {
                        lnodes[l].downs.iter().map(|&s| pos[segs[s].down]).collect()
                    };
                    let key = if nb.is_empty() {
                        pos[l] as f64
                    } else {
                        nb.iter().sum::<usize>() as f64 / nb.len() as f64
                    };
                    (l, key)
                })
                .collect();
            group_sort(&mut items, &lnodes, 0);
            ranks[r] = items.into_iter().map(|(l, _)| l).collect();
            for (i, &l) in ranks[r].iter().enumerate() {
                pos[l] = i;
            }
        }
        let c = count_crossings(&ranks, &lnodes, &segs, &pos);
        if c < best_cross {
            best_cross = c;
            best = ranks.clone();
            stale = 0;
        } else {
            stale += 1;
            if stale >= 4 {
                break;
            }
        }
    }
    let ranks = best;
    set_pos(&ranks, &mut pos);

    // ── Ports ──
    // A real node's edges on one side share a port when they look the same
    // at that end (marker, line style, cycle-reversed or not); different
    // kinds get their own ports, two cells apart.
    for v in 0..n {
        // Groups per side: (down side?, ordered list of segment groups).
        let mut sides: Vec<(bool, Vec<Vec<usize>>)> = Vec::new();
        for down_side in [true, false] {
            let list: &Vec<usize> = if down_side {
                &lnodes[v].downs
            } else {
                &lnodes[v].ups
            };
            if list.is_empty() {
                continue;
            }
            let mut groups: Vec<((Marker, bool, LineStyle), Vec<usize>)> = Vec::new();
            for &s in list {
                let ei = segs[s].edge;
                let e = &g.edges[ei];
                let inf = info(ei);
                let marker = end_marker(e.start, e.end, inf.rev, down_side);
                let key = (marker, inf.cyc, e.style);
                match groups.iter_mut().find(|(k, _)| *k == key) {
                    Some((_, v)) => v.push(s),
                    None => groups.push((key, vec![s])),
                }
            }
            let mut groups: Vec<Vec<usize>> = groups.into_iter().map(|(_, v)| v).collect();
            if g.nodes[v].shape.is_glyph() {
                // Too small for more than one port.
                groups = vec![groups.concat()];
            }
            // Order ports by where the other ends sit.
            let key = |ss: &Vec<usize>| {
                ss.iter()
                    .map(|&s| pos[if down_side { segs[s].down } else { segs[s].up }] as f64)
                    .sum::<f64>()
                    / ss.len() as f64
            };
            groups.sort_by(|a, b| key(a).total_cmp(&key(b)));
            sides.push((down_side, groups));
        }
        let most = sides.iter().map(|(_, gs)| gs.len()).max().unwrap_or(0);
        let b = &mut boxes[v];
        let size = if horiz { &mut b.h } else { &mut b.w };
        if !g.nodes[v].shape.is_glyph() && *size < 2 * most + 1 {
            *size = 2 * most + 1;
        }
        let center = (*size - 1) / 2;
        for (down_side, groups) in sides {
            let ng = groups.len();
            for (k, ss) in groups.into_iter().enumerate() {
                let off = (center + 2 * k).saturating_sub(ng - 1);
                for s in ss {
                    if down_side {
                        segs[s].port_up = off;
                    } else {
                        segs[s].port_down = off;
                    }
                }
            }
        }
    }

    // ── Extents ──
    for ln in lnodes.iter_mut() {
        match ln.kind {
            Kind::Real(v) => {
                let b = &boxes[v];
                let has_loop = b.loop_label.is_some();
                let lw = b.loop_label.as_deref().map_or(0, text::width);
                if horiz {
                    ln.cross = b.h + usize::from(has_loop);
                    ln.main = if has_loop && lw > 0 {
                        b.w.max(5 + lw)
                    } else {
                        b.w
                    };
                } else {
                    ln.cross = b.w
                        + if has_loop {
                            if lw > 0 {
                                3 + lw
                            } else {
                                2
                            }
                        } else {
                            0
                        };
                    ln.main = b.h;
                }
            }
            Kind::Dummy => {
                ln.cross = 1;
                ln.main = 0;
            }
            Kind::Label => {
                let lines = &info(ln.edge).label;
                let lw = lines.iter().map(|l| text::width(l)).max().unwrap_or(0);
                if horiz {
                    ln.cross = lines.len();
                    ln.main = lw + 2;
                } else {
                    ln.cross = lw + 2;
                    ln.main = lines.len();
                }
            }
        }
    }
    let port_of = |l: usize, s: usize| -> f64 {
        let seg = &segs[s];
        match lnodes[l].kind {
            Kind::Real(_) => {
                (if seg.up == l {
                    seg.port_up
                } else {
                    seg.port_down
                }) as f64
            }
            Kind::Dummy => 0.0,
            Kind::Label => ((lnodes[l].cross - 1) / 2) as f64,
        }
    };

    // ── Cross-axis coordinates ──
    let sep_between = |a: usize, b: usize| -> usize {
        let (pa, pb) = (&lnodes[a].cluster_path, &lnodes[b].cluster_path);
        let common = pa.iter().zip(pb.iter()).take_while(|(x, y)| x == y).count();
        let frames = (pa.len() - common) + (pb.len() - common);
        let base = if lnodes[a].kind == Kind::Dummy || lnodes[b].kind == Kind::Dummy {
            1
        } else {
            p.sep
        };
        base + 2 * frames
    };
    let mut x = vec![0f64; lnodes.len()];
    for rk in &ranks {
        let mut cur = 0usize;
        for (i, &l) in rk.iter().enumerate() {
            if i > 0 {
                cur += sep_between(rk[i - 1], l);
            }
            x[l] = cur as f64;
            cur += lnodes[l].cross;
        }
    }
    let weight = |a: usize, b: usize| -> f64 {
        let real = |l: usize| matches!(lnodes[l].kind, Kind::Real(_));
        match (real(a), real(b)) {
            (true, true) => 1.0,
            (false, false) => 8.0,
            _ => 2.0,
        }
    };
    let place = |x: &mut Vec<f64>, rk: &[usize], use_up: bool, use_down: bool| {
        if rk.is_empty() {
            return;
        }
        let mut desired = Vec::with_capacity(rk.len());
        let mut weights = Vec::with_capacity(rk.len());
        let mut offsets = Vec::with_capacity(rk.len());
        let mut off = 0usize;
        for (i, &l) in rk.iter().enumerate() {
            if i > 0 {
                off += lnodes[rk[i - 1]].cross + sep_between(rk[i - 1], l);
            }
            let mut sw = 0.0;
            let mut sx = 0.0;
            let mut add = |s: usize, other: usize| {
                let w = weight(l, other);
                sw += w;
                sx += w * (x[other] + port_of(other, s) - port_of(l, s));
            };
            if use_up {
                for &s in &lnodes[l].ups {
                    add(s, segs[s].up);
                }
            }
            if use_down {
                for &s in &lnodes[l].downs {
                    add(s, segs[s].down);
                }
            }
            if sw == 0.0 {
                desired.push(x[l] - off as f64);
                weights.push(0.1);
            } else {
                desired.push(sx / sw - off as f64);
                weights.push(sw);
            }
            offsets.push(off as f64);
        }
        let y = pava(&desired, &weights);
        for (i, &l) in rk.iter().enumerate() {
            x[l] = y[i] + offsets[i];
        }
    };
    let coord_iters = if lnodes.len() > 1500 { 3 } else { 8 };
    for _ in 0..coord_iters {
        for rk in ranks.iter().skip(1) {
            place(&mut x, rk, true, false);
        }
        for rk in ranks.iter().rev().skip(1) {
            place(&mut x, rk, false, true);
        }
    }
    // Last pass top-down in whole columns: each rank lines up under the
    // (already integer) rank above, so chains come out straight.
    let mut xi = vec![0i64; lnodes.len()];
    for (r, rk) in ranks.iter().enumerate() {
        if r > 0 {
            place(&mut x, rk, true, false);
        }
        for (i, &l) in rk.iter().enumerate() {
            let want = x[l].round() as i64;
            xi[l] = if i == 0 {
                want
            } else {
                let prev = rk[i - 1];
                want.max(xi[prev] + (lnodes[prev].cross + sep_between(prev, l)) as i64)
            };
            x[l] = xi[l] as f64;
        }
    }

    // Straighten: move a node sideways (pushing its neighbours along when
    // they are in the way) when that lines up more edges than it bends —
    // rounding and packing leave one-column jogs.
    let node_cost = |xi: &Vec<i64>, l: usize, at: i64| -> f64 {
        lnodes[l]
            .ups
            .iter()
            .map(|&s| (s, segs[s].up))
            .chain(lnodes[l].downs.iter().map(|&s| (s, segs[s].down)))
            .map(|(s, o)| {
                let a = at as f64 + port_of(l, s);
                let b = xi[o] as f64 + port_of(o, s);
                // Ties go to a straight arrival at the lower node.
                let bias = if segs[s].up == l && !matches!(lnodes[l].kind, Kind::Real(_)) {
                    1.01
                } else {
                    1.0
                };
                bias * weight(l, o) * (a - b).abs()
            })
            .sum()
    };
    for _ in 0..4 {
        let mut moved = false;
        for rk in &ranks {
            for i in 0..rk.len() {
                let l = rk[i];
                let targets: Vec<i64> = lnodes[l]
                    .ups
                    .iter()
                    .map(|&s| (s, segs[s].up))
                    .chain(lnodes[l].downs.iter().map(|&s| (s, segs[s].down)))
                    .map(|(s, o)| (xi[o] as f64 + port_of(o, s) - port_of(l, s)).round() as i64)
                    .collect();
                let mut best: Option<(f64, Vec<(usize, i64)>)> = None;
                for at in targets {
                    if at == xi[l] || at < 0 {
                        continue;
                    }
                    // The nodes that move, with their new positions.
                    let mut moves = vec![(l, at)];
                    if at > xi[l] {
                        let mut edge = at + lnodes[l].cross as i64;
                        for j in i + 1..rk.len() {
                            let q = rk[j];
                            let need = edge + sep_between(rk[j - 1], q) as i64;
                            if xi[q] >= need {
                                break;
                            }
                            moves.push((q, need));
                            edge = need + lnodes[q].cross as i64;
                        }
                    } else {
                        let mut left = at;
                        for j in (0..i).rev() {
                            let q = rk[j];
                            let need = left - (lnodes[q].cross + sep_between(q, rk[j + 1])) as i64;
                            if xi[q] <= need {
                                break;
                            }
                            moves.push((q, need));
                            left = need;
                        }
                    }
                    let delta: f64 = moves
                        .iter()
                        .map(|&(q, to)| node_cost(&xi, q, to) - node_cost(&xi, q, xi[q]))
                        .sum();
                    if delta < -1e-9 && best.as_ref().is_none_or(|(d, _)| delta < *d) {
                        best = Some((delta, moves));
                    }
                }
                if let Some((_, moves)) = best {
                    for (q, to) in moves {
                        xi[q] = to;
                    }
                    moved = true;
                }
            }
        }
        if !moved {
            break;
        }
    }

    // ── Subgraph frames ──
    let nclusters = g.clusters.len();
    let depth_of = |c: usize| g.cluster_path(Some(c)).len();
    let mut by_depth: Vec<usize> = (0..nclusters).collect();
    by_depth.sort_by_key(|&c| std::cmp::Reverse(depth_of(c)));
    let title_w = |c: usize| {
        let cl = &g.clusters[c];
        text::width(if cl.title.is_empty() {
            &cl.id
        } else {
            &cl.title
        })
    };
    // Per cluster: cross range and rank range, built from the innermost out.
    // `widen` makes room for the title in top-down layouts.
    let compute = |widen: bool| -> Vec<Option<(i64, i64, usize, usize)>> {
        let mut frame: Vec<Option<(i64, i64, usize, usize)>> = vec![None; nclusters];
        for &c in &by_depth {
            let mut range: Option<(i64, i64, usize, usize)> = None;
            let grow =
                |r: &mut Option<(i64, i64, usize, usize)>, a: i64, b: i64, r0: usize, r1: usize| {
                    *r = Some(match *r {
                        None => (a, b, r0, r1),
                        Some((x0, x1, s0, s1)) => (x0.min(a), x1.max(b), s0.min(r0), s1.max(r1)),
                    });
                };
            for (l, ln) in lnodes.iter().enumerate() {
                if ln.cluster_path.last() == Some(&c) {
                    grow(
                        &mut range,
                        xi[l],
                        xi[l] + ln.cross as i64 - 1,
                        ln.rank,
                        ln.rank,
                    );
                }
            }
            for (d, cl) in g.clusters.iter().enumerate() {
                if cl.parent == Some(c) && d != c {
                    if let Some((a, b, r0, r1)) = frame[d] {
                        grow(&mut range, a, b, r0, r1);
                    }
                }
            }
            frame[c] = range.map(|(a, b, r0, r1)| {
                let (a, mut b) = (a - 2, b + 2);
                if widen && !horiz {
                    b = b.max(a + title_w(c).min(40) as i64 + 5);
                }
                (a, b, r0, r1)
            });
        }
        frame
    };
    let in_cluster = |l: usize, c: usize| lnodes[l].cluster_path.contains(&c);
    let valid = |frame: &[Option<(i64, i64, usize, usize)>]| -> bool {
        for c in 0..nclusters {
            let Some((c0, c1, r0, r1)) = frame[c] else {
                continue;
            };
            for (l, ln) in lnodes.iter().enumerate() {
                if ln.rank < r0 || ln.rank > r1 || in_cluster(l, c) {
                    continue;
                }
                let (a, b) = (xi[l], xi[l] + ln.cross as i64 - 1);
                let hit = match ln.kind {
                    Kind::Dummy => a == c0 || a == c1,
                    _ => a <= c1 && b >= c0,
                };
                if hit {
                    return false;
                }
            }
            let pc = g.cluster_path(Some(c));
            for (d, fd) in frame.iter().enumerate().skip(c + 1) {
                let Some((d0, d1, s0, s1)) = *fd else {
                    continue;
                };
                if pc.contains(&d) || g.cluster_path(Some(d)).contains(&c) {
                    continue;
                }
                if s0 <= r1 && r0 <= s1 && d0 <= c1 && c0 <= d1 {
                    return false;
                }
            }
        }
        true
    };
    let mut frame = compute(true);
    let mut frames_ok = valid(&frame);
    if !frames_ok {
        frame = compute(false);
        frames_ok = valid(&frame);
    }
    let mut notes = Vec::new();
    if !frames_ok {
        for (c, cl) in g.clusters.iter().enumerate() {
            let members: Vec<String> = (0..n)
                .filter(|&v| node_path[v].contains(&c))
                .map(|v| g.nodes[v].label.replace('\n', " "))
                .collect();
            if !members.is_empty() {
                let title = if cl.title.is_empty() {
                    &cl.id
                } else {
                    &cl.title
                };
                notes.push(format!(
                    "{}: {}",
                    title.replace('\n', " "),
                    members.join(", ")
                ));
            }
        }
        frame.iter_mut().for_each(|f| *f = None);
    }

    // Shift so everything starts at cross 0.
    let min_x = lnodes
        .iter()
        .enumerate()
        .map(|(l, _)| xi[l])
        .chain(frame.iter().flatten().map(|f| f.0))
        .min()
        .unwrap_or(0);
    for v in xi.iter_mut() {
        *v -= min_x;
    }
    for f in frame.iter_mut().flatten() {
        f.0 -= min_x;
        f.1 -= min_x;
    }
    let cross_total = lnodes
        .iter()
        .enumerate()
        .map(|(l, ln)| xi[l] as usize + ln.cross)
        .chain(frame.iter().flatten().map(|f| f.1 as usize + 1))
        .max()
        .unwrap_or(1);
    let xs: Vec<usize> = xi.iter().map(|&v| v.max(0) as usize).collect();

    // Width check before the expensive part, for top-down layouts.
    if !horiz && cross_total > avail {
        return Err(Fail::TooWide(cross_total));
    }

    // ── Routing tracks per gap ──
    let seg_cols = |s: usize| -> (usize, usize) {
        let sg = &segs[s];
        (
            xs[sg.up] + port_of(sg.up, s) as usize,
            xs[sg.down] + port_of(sg.down, s) as usize,
        )
    };
    let mut gap_segs: Vec<Vec<usize>> = vec![Vec::new(); nranks.saturating_sub(1)];
    for (s, sg) in segs.iter().enumerate() {
        gap_segs[lnodes[sg.up].rank].push(s);
    }
    let mut track_of = vec![0usize; segs.len()];
    let mut ntracks = vec![0usize; gap_segs.len()];
    for (gi, list) in gap_segs.iter().enumerate() {
        // Nets share a horizontal track: segments leaving the same port
        // (fan-out), and single segments from different ports arriving at
        // the same port (fan-in).
        let mut nets: Vec<Vec<usize>> = Vec::new();
        for &s in list {
            let (cu, _) = seg_cols(s);
            match nets
                .iter_mut()
                .find(|ss| seg_cols(ss[0]).0 == cu && segs[ss[0]].up == segs[s].up)
            {
                Some(ss) => ss.push(s),
                None => nets.push(vec![s]),
            }
        }
        let mut merged: Vec<Vec<usize>> = Vec::new();
        for net in nets {
            let single = net.len() == 1;
            let target = |s: usize| (segs[s].down, seg_cols(s).1);
            let into = if single {
                merged.iter_mut().find(|m| {
                    m.iter().all(|&t| target(t) == target(net[0]))
                        && m.iter().all(|&t| segs[t].up != segs[net[0]].up)
                })
            } else {
                None
            };
            match into {
                Some(m) => m.push(net[0]),
                None => merged.push(net),
            }
        }
        // A merged group must be all single fan-in segments or one fan-out
        // net; a fan-out net never absorbs others (the find above only
        // matches groups whose every segment has the same target).
        let nets = merged;
        let cols_of = |ss: &Vec<usize>| -> (Vec<usize>, Vec<usize>) {
            let mut srcs: Vec<usize> = ss.iter().map(|&s| seg_cols(s).0).collect();
            let mut dsts: Vec<usize> = ss.iter().map(|&s| seg_cols(s).1).collect();
            srcs.sort_unstable();
            srcs.dedup();
            dsts.sort_unstable();
            dsts.dedup();
            (srcs, dsts)
        };
        let info_net: Vec<(usize, usize, Vec<usize>, Vec<usize>)> = nets
            .iter()
            .map(|ss| {
                let (srcs, dsts) = cols_of(ss);
                let lo = srcs.iter().chain(dsts.iter()).copied().min().unwrap_or(0);
                let hi = srcs.iter().chain(dsts.iter()).copied().max().unwrap_or(0);
                (lo, hi, srcs, dsts)
            })
            .collect();
        let routed: Vec<usize> = (0..nets.len())
            .filter(|&i| info_net[i].0 != info_net[i].1)
            .collect();
        // Net a must sit above net b when one of a's source columns is one
        // of b's drop columns (otherwise the two verticals would overlap).
        let mut preds: Vec<Vec<usize>> = vec![Vec::new(); nets.len()];
        for &a in &routed {
            for &b in &routed {
                if a != b && info_net[a].2.iter().any(|c| info_net[b].3.contains(c)) {
                    preds[b].push(a);
                }
            }
        }
        let mut order: Vec<usize> = Vec::new();
        let mut done = vec![false; nets.len()];
        let mut remaining: Vec<usize> = routed.clone();
        remaining.sort_by_key(|&i| (info_net[i].0, info_net[i].1));
        while !remaining.is_empty() {
            let pick = remaining
                .iter()
                .position(|&i| preds[i].iter().all(|&q| done[q]))
                .unwrap_or(0);
            let i = remaining.remove(pick);
            done[i] = true;
            order.push(i);
        }
        let mut track = vec![usize::MAX; nets.len()];
        let mut used: Vec<Vec<(usize, usize)>> = Vec::new();
        for &i in &order {
            let lo_bound = preds[i]
                .iter()
                .filter(|&&q| track[q] != usize::MAX)
                .map(|&q| track[q] + 1)
                .max()
                .unwrap_or(0);
            let (a, b) = (info_net[i].0, info_net[i].1);
            let mut t = lo_bound;
            loop {
                if t >= used.len() {
                    used.resize(t + 1, Vec::new());
                }
                if used[t].iter().all(|&(c, d)| b + 1 < c || d + 1 < a) {
                    break;
                }
                t += 1;
            }
            used[t].push((a, b));
            track[i] = t;
            for &s in &nets[i] {
                track_of[s] = t;
            }
        }
        ntracks[gi] = used.len();
    }

    // ── Main-axis positions ──
    let mut rank_ext = vec![0usize; nranks];
    for ln in &lnodes {
        rank_ext[ln.rank] = rank_ext[ln.rank].max(ln.main);
    }
    // A gap needs its last row for arrowheads (and as spacing) unless its
    // tracks already separate the ranks and nothing below is a real node.
    let needs_arrow_row = |gi: usize| {
        ntracks[gi] == 0
            || gap_segs[gi]
                .iter()
                .any(|&s| matches!(lnodes[segs[s].down].kind, Kind::Real(_)))
    };
    let frame_list: Vec<usize> = (0..nclusters).filter(|&c| frame[c].is_some()).collect();
    let fr = |c: usize| frame[c].expect("frame");
    let mut frame_top = vec![0usize; nclusters];
    let mut frame_bottom = vec![0usize; nclusters];
    let mut rank_start = vec![0usize; nranks];
    let mut track_start = vec![0usize; gap_segs.len()];
    let mut cursor = 0usize;
    // Frames starting (ending) at a rank, grouped into one row per nesting
    // depth: siblings share a row, an outer frame's row comes first on the
    // outside.
    let rows_at = |r: usize, top: bool| -> Vec<Vec<usize>> {
        let mut v: Vec<(usize, usize)> = frame_list
            .iter()
            .copied()
            .filter(|&c| if top { fr(c).2 == r } else { fr(c).3 == r })
            .map(|c| (depth_of(c), c))
            .collect();
        v.sort();
        if !top {
            v.reverse();
        }
        let mut out: Vec<Vec<usize>> = Vec::new();
        let mut last = usize::MAX;
        for (d, c) in v {
            if d != last {
                out.push(Vec::new());
                last = d;
            }
            if let Some(row) = out.last_mut() {
                row.push(c);
            }
        }
        out
    };
    // A row at the top of the gap for markers at upper nodes, and to show
    // the line leaving a bare glyph (`●`) before it turns.
    let has_upper_marker = |gi: usize| {
        gap_segs[gi].iter().any(|&s| {
            let sg = &segs[s];
            let Kind::Real(v) = lnodes[sg.up].kind else {
                return false;
            };
            let e = &g.edges[sg.edge];
            end_marker(e.start, e.end, info(sg.edge).rev, true) != Marker::None
                || (g.nodes[v].shape.is_glyph() && {
                    let (a, b) = seg_cols(s);
                    a != b
                })
        })
    };
    for row in rows_at(0, true) {
        for c in row {
            frame_top[c] = cursor;
        }
        cursor += 1;
    }
    for r in 0..nranks {
        rank_start[r] = cursor;
        cursor += rank_ext[r];
        if r + 1 < nranks {
            if has_upper_marker(r) {
                cursor += 1;
            }
            for row in rows_at(r, false) {
                for c in row {
                    frame_bottom[c] = cursor;
                }
                cursor += 1;
            }
            track_start[r] = cursor;
            cursor += ntracks[r];
            for row in rows_at(r + 1, true) {
                for c in row {
                    frame_top[c] = cursor;
                }
                cursor += 1;
            }
            if needs_arrow_row(r) {
                cursor += 1;
            }
        } else {
            for row in rows_at(r, false) {
                for c in row {
                    frame_bottom[c] = cursor;
                }
                cursor += 1;
            }
        }
    }
    let main_total = cursor;
    let (rows, cols) = if horiz {
        (cross_total, main_total)
    } else {
        (main_total, cross_total)
    };
    if cols > avail {
        return Err(Fail::TooWide(cols));
    }
    if rows.saturating_mul(cols) > MAX_CELLS {
        return Err(Fail::TooBig);
    }
    let at = |m: usize, c: usize| -> (usize, usize) {
        if horiz {
            (c, m)
        } else {
            (m, c)
        }
    };
    let mut cv = Canvas::new(rows, cols);

    // ── Draw: frames ──
    for &c in &frame_list {
        let (c0, c1, _, _) = fr(c);
        let (m0, m1) = (frame_top[c], frame_bottom[c]);
        let (c0, c1) = (c0 as usize, c1 as usize);
        let corners = [at(m0, c0), at(m0, c1), at(m1, c1), at(m1, c0), at(m0, c0)];
        cv.path(&corners, LineStyle::Solid, Class::Frame);
    }

    // ── Draw: edges ──
    let mut markers: Vec<(usize, usize, &'static str)> = Vec::new();
    for (s, sg) in segs.iter().enumerate() {
        let e = &g.edges[sg.edge];
        let inf = info(sg.edge);
        let (cu, cl) = seg_cols(s);
        let gi = lnodes[sg.up].rank;
        if e.style == LineStyle::Invisible {
            continue;
        }
        let start = match lnodes[sg.up].kind {
            Kind::Real(v) => {
                let border = rank_start[gi] + if horiz { boxes[v].w } else { boxes[v].h } - 1;
                let mk = end_marker(e.start, e.end, inf.rev, true);
                if mk != Marker::None {
                    markers.push((border + 1, cu, marker_glyph(mk, horiz, false)));
                }
                if mk != Marker::None || g.nodes[v].shape.is_glyph() {
                    border + 1
                } else {
                    border
                }
            }
            _ => rank_start[gi],
        };
        let entry = rank_start[gi + 1];
        let end = match lnodes[sg.down].kind {
            Kind::Real(v) => {
                let mk = end_marker(e.start, e.end, inf.rev, false);
                if mk != Marker::None {
                    markers.push((entry - 1, cl, marker_glyph(mk, horiz, true)));
                }
                if mk != Marker::None || g.nodes[v].shape.is_glyph() {
                    entry - 1
                } else {
                    entry
                }
            }
            _ => entry,
        };
        let pts: Vec<(usize, usize)> = if cu == cl {
            vec![at(start, cu), at(end, cl)]
        } else {
            let t = track_start[gi] + track_of[s];
            vec![at(start, cu), at(t, cu), at(t, cl), at(end, cl)]
        };
        cv.path(&pts, e.style, Class::Edge);
    }

    // ── Draw: nodes ──
    for (l, ln) in lnodes.iter().enumerate() {
        let Kind::Real(v) = ln.kind else { continue };
        let b = &boxes[v];
        let (r, c) = at(rank_start[ln.rank], xs[l]);
        draw_box(&mut cv, r, c, b, g.nodes[v].shape, horiz);
        if let Some(lbl) = &b.loop_label {
            if !g.nodes[v].shape.is_glyph() {
                if horiz {
                    let (br, bc) = (r + b.h - 1, c);
                    cv.path(
                        &[
                            (br, bc + 1),
                            (br + 1, bc + 1),
                            (br + 1, bc + 3),
                            (br, bc + 3),
                        ],
                        LineStyle::Solid,
                        Class::Edge,
                    );
                    cv.glyph(br, bc + 3, "▲", Class::Edge);
                    cv.text(br + 1, bc + 5, lbl, cols, Class::Label);
                } else {
                    let rc = c + b.w - 1;
                    cv.path(
                        &[
                            (r + 1, rc),
                            (r + 1, rc + 2),
                            (r + 2, rc + 2),
                            (r + 2, rc + 1),
                        ],
                        LineStyle::Solid,
                        Class::Edge,
                    );
                    cv.glyph(r + 2, rc + 1, "◀", Class::Edge);
                    cv.text(r + 1, rc + 4, lbl, cols, Class::Label);
                }
            }
        }
    }
    for (m, c, glyph) in markers {
        let (r, cc) = at(m, c);
        cv.glyph(r, cc, glyph, Class::Edge);
    }

    // ── Draw: edge labels ──
    for (l, ln) in lnodes.iter().enumerate() {
        if ln.kind != Kind::Label {
            continue;
        }
        let lines = &info(ln.edge).label;
        let lw = lines.iter().map(|s| text::width(s)).max().unwrap_or(0);
        for (i, line) in lines.iter().enumerate() {
            let pad = lw - text::width(line);
            let body = format!(
                "{}{}{}",
                " ".repeat(pad / 2),
                line,
                " ".repeat(pad - pad / 2)
            );
            if horiz {
                let (r, c) = (xs[l] + i, rank_start[ln.rank]);
                cv.text(r, c, &format!(" {body} "), cols, Class::Label);
            } else {
                let (r, c) = (rank_start[ln.rank] + i, xs[l] + 1);
                cv.text(r, c, &body, cols, Class::Label);
            }
        }
    }

    // ── Draw: frame titles ──
    for &c in &frame_list {
        let (c0, c1, _, _) = fr(c);
        let cl = &g.clusters[c];
        let title = if cl.title.is_empty() {
            &cl.id
        } else {
            &cl.title
        };
        let title = title.replace('\n', " ");
        // Top border first, else the bottom one; on either, the first
        // stretch of plain border (edges cross frames) that fits the title,
        // else the longest.
        let (rows_try, col, room) = if horiz {
            (
                [c0 as usize, c1 as usize],
                frame_top[c] + 2,
                frame_bottom[c].saturating_sub(frame_top[c] + 3),
            )
        } else {
            (
                [frame_top[c], frame_bottom[c]],
                c0 as usize + 2,
                ((c1 - c0) as usize).saturating_sub(3),
            )
        };
        let want = text::width(&title) + 2;
        let mut best = (0usize, col, rows_try[0]);
        for r in rows_try {
            let mut run_start = col;
            for cc in col..=col + room {
                if cc < col + room && cv.plain_h(r, cc) {
                    continue;
                }
                let len = cc - run_start;
                if len > best.0 && best.0 < want {
                    best = (len, run_start, r);
                }
                run_start = cc + 1;
            }
        }
        let (len, at, r) = best;
        if len >= 3 {
            let t = text::truncate(&title, len.min(want) - 2);
            cv.text(r, at, &format!(" {t} "), at + len, Class::Title);
        }
    }

    Ok(Drawn {
        rows: cv.render(),
        notes,
    })
}

/// The marker drawn where an edge meets the node at its top end
/// (`at_top`) or bottom end.
fn end_marker(start: Marker, end: Marker, rev: bool, at_top: bool) -> Marker {
    // Not reversed: `from` is at the top.
    if at_top != rev {
        start
    } else {
        end
    }
}

fn marker_glyph(m: Marker, horiz: bool, forward: bool) -> &'static str {
    match (m, horiz, forward) {
        (Marker::Arrow, false, true) => "▼",
        (Marker::Arrow, false, false) => "▲",
        (Marker::Arrow, true, true) => "▶",
        (Marker::Arrow, true, false) => "◀",
        (Marker::Triangle, false, true) => "▽",
        (Marker::Triangle, false, false) => "△",
        (Marker::Triangle, true, true) => "▷",
        (Marker::Triangle, true, false) => "◁",
        (Marker::DiamondFilled, ..) => "◆",
        (Marker::DiamondOpen, ..) => "◇",
        (Marker::Circle, ..) => "○",
        (Marker::Cross, ..) => "×",
        (Marker::None, ..) => "",
    }
}

/// Size a node's box and wrap its text.
fn node_box(
    shape: Shape,
    label: &str,
    sections: &[Vec<String>],
    wrap: usize,
    horiz: bool,
    loops: &[String],
) -> NodeBox {
    let loop_label = if loops.is_empty() {
        None
    } else {
        let named: Vec<&str> = loops
            .iter()
            .map(String::as_str)
            .filter(|l| !l.is_empty())
            .collect();
        Some(text::truncate(&named.join(", "), wrap.max(6)))
    };
    let (w, h, lines, secs) = match shape {
        Shape::Start | Shape::End | Shape::Choice => (1, 1, Vec::new(), Vec::new()),
        Shape::Bar => {
            if horiz {
                (1, 3, Vec::new(), Vec::new())
            } else {
                (5, 1, Vec::new(), Vec::new())
            }
        }
        Shape::Sections => {
            let lines = text::wrap(label, wrap);
            let member_wrap = wrap + wrap / 2;
            let secs: Vec<Vec<String>> = sections
                .iter()
                .filter(|s| !s.is_empty())
                .map(|s| {
                    s.iter()
                        .flat_map(|row| text::wrap(row, member_wrap))
                        .collect()
                })
                .collect();
            let tw = lines
                .iter()
                .chain(secs.iter().flatten())
                .map(|l| text::width(l))
                .max()
                .unwrap_or(1);
            let th = lines.len() + secs.iter().map(|s| s.len() + 1).sum::<usize>();
            (tw + 4, th + 2, lines, secs)
        }
        _ => {
            let lines = text::wrap(label, wrap);
            let tw = lines
                .iter()
                .map(|l| text::width(l))
                .max()
                .unwrap_or(1)
                .max(1);
            let extra = usize::from(shape == Shape::Cylinder);
            (tw + 4, lines.len() + 2 + extra, lines, Vec::new())
        }
    };
    let (mut w, mut h) = (w, h);
    if loop_label.is_some() && !shape.is_glyph() {
        if horiz {
            w = w.max(5);
        } else {
            h = h.max(4);
        }
    }
    NodeBox {
        w,
        h,
        lines,
        sections: secs,
        loop_label,
    }
}

/// Draw a node box with its text at screen position (r, c).
fn draw_box(cv: &mut Canvas, r: usize, c: usize, b: &NodeBox, shape: Shape, horiz: bool) {
    let (w, h) = (b.w, b.h);
    match shape {
        Shape::Start => return cv.glyph(r, c, "●", Class::Node),
        Shape::End => return cv.glyph(r, c, "◉", Class::Node),
        Shape::Choice => return cv.glyph(r, c, "◇", Class::Node),
        Shape::Bar => {
            for i in 0..w.max(h) {
                if horiz {
                    cv.glyph(r + i, c, "┃", Class::Node);
                } else {
                    cv.glyph(r, c + i, "━", Class::Node);
                }
            }
            return;
        }
        _ => {}
    }
    let (r1, c1) = (r + h - 1, c + w - 1);
    if shape == Shape::Subroutine {
        for cc in c + 1..c1 {
            cv.add_double(r, cc, LEFT | RIGHT, Class::Node);
            cv.add_double(r1, cc, LEFT | RIGHT, Class::Node);
        }
        for rr in r + 1..r1 {
            cv.add_double(rr, c, UP | DOWN, Class::Node);
            cv.add_double(rr, c1, UP | DOWN, Class::Node);
        }
    } else {
        cv.line(r, c, r, c1, LineStyle::Solid, Class::Node);
        cv.line(r1, c, r1, c1, LineStyle::Solid, Class::Node);
        cv.line(r, c, r1, c, LineStyle::Solid, Class::Node);
        cv.line(r, c1, r1, c1, LineStyle::Solid, Class::Node);
    }
    let corners: [&str; 4] = match shape {
        Shape::Round | Shape::Stadium | Shape::Circle | Shape::Cylinder => ["╭", "╮", "╰", "╯"],
        Shape::Diamond | Shape::Hexagon => ["╱", "╲", "╲", "╱"],
        Shape::Subroutine => ["╔", "╗", "╚", "╝"],
        _ => ["┌", "┐", "└", "┘"],
    };
    cv.glyph(r, c, corners[0], Class::Node);
    cv.glyph(r, c1, corners[1], Class::Node);
    cv.glyph(r1, c, corners[2], Class::Node);
    cv.glyph(r1, c1, corners[3], Class::Node);
    let mid = r + (h - 1) / 2;
    match shape {
        Shape::Stadium | Shape::Circle => {
            for rr in r + 1..r1 {
                cv.glyph(rr, c, "(", Class::Node);
                cv.glyph(rr, c1, ")", Class::Node);
            }
        }
        Shape::Diamond | Shape::Hexagon => {
            cv.glyph(mid, c, "<", Class::Node);
            cv.glyph(mid, c1, ">", Class::Node);
        }
        Shape::Flag => cv.glyph(mid, c, ">", Class::Node),
        Shape::Slant(a, z) => {
            for rr in r + 1..r1 {
                cv.glyph(rr, c, &a.to_string(), Class::Node);
                cv.glyph(rr, c1, &z.to_string(), Class::Node);
            }
        }
        _ => {}
    }
    let inner = w - 2;
    let center = |cv: &mut Canvas, row: usize, s: &str, class: Class| {
        let off = (inner.saturating_sub(text::width(s))) / 2;
        cv.text(row, c + 1 + off, s, c1, class);
    };
    if shape == Shape::Sections {
        let mut row = r + 1;
        for l in &b.lines {
            center(cv, row, l, Class::Title);
            row += 1;
        }
        for sec in &b.sections {
            cv.add_bits(row, c, RIGHT, LineStyle::Solid, Class::Node);
            cv.add_bits(row, c1, LEFT, LineStyle::Solid, Class::Node);
            cv.line(row, c, row, c1, LineStyle::Solid, Class::Node);
            row += 1;
            for l in sec {
                cv.text(row, c + 2, l, c1, Class::Text);
                row += 1;
            }
        }
        return;
    }
    let mut top = r + 1;
    let mut interior = h - 2;
    if shape == Shape::Cylinder {
        cv.line(r + 1, c, r + 1, c1, LineStyle::Solid, Class::Node);
        top += 1;
        interior -= 1;
    }
    let start = top + (interior - b.lines.len().min(interior)) / 2;
    for (i, l) in b.lines.iter().enumerate() {
        center(cv, start + i, l, Class::Text);
    }
}

/// Weighted isotonic regression (pool adjacent violators): the
/// non-decreasing sequence closest to `d` in weighted least squares.
fn pava(d: &[f64], w: &[f64]) -> Vec<f64> {
    // (weighted sum, total weight, count)
    let mut blocks: Vec<(f64, f64, usize)> = Vec::with_capacity(d.len());
    for i in 0..d.len() {
        blocks.push((d[i] * w[i], w[i], 1));
        while blocks.len() >= 2 {
            let (s1, w1, n1) = blocks[blocks.len() - 1];
            let (s0, w0, n0) = blocks[blocks.len() - 2];
            if s0 / w0 > s1 / w1 {
                blocks.pop();
                let last = blocks.len() - 1;
                blocks[last] = (s0 + s1, w0 + w1, n0 + n1);
            } else {
                break;
            }
        }
    }
    let mut out = Vec::with_capacity(d.len());
    for (s, w, n) in blocks {
        out.extend(std::iter::repeat_n(s / w, n));
    }
    out
}

/// Layout nodes with their sort keys.
type Keyed = Vec<(usize, f64)>;

/// Sort a rank by key, keeping every subgraph's members contiguous: groups
/// (a subgraph, or a single node outside any) are ordered by their mean key,
/// and members recursively within their group.
fn group_sort(items: &mut Keyed, lnodes: &[LNode], depth: usize) {
    if items.len() <= 1 {
        return;
    }
    // Group by cluster at this depth; nodes without one are singletons.
    let mut groups: Vec<(Option<usize>, Keyed)> = Vec::new();
    for &(l, k) in items.iter() {
        let c = lnodes[l].cluster_path.get(depth).copied();
        match c {
            Some(c) => match groups.iter_mut().find(|(g, _)| *g == Some(c)) {
                Some((_, v)) => v.push((l, k)),
                None => groups.push((Some(c), vec![(l, k)])),
            },
            None => groups.push((None, vec![(l, k)])),
        }
    }
    let mut keyed: Vec<(f64, usize, Option<usize>, Keyed)> = groups
        .into_iter()
        .enumerate()
        .map(|(i, (c, v))| {
            let k = v.iter().map(|(_, k)| k).sum::<f64>() / v.len() as f64;
            (k, i, c, v)
        })
        .collect();
    keyed.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    items.clear();
    for (_, _, c, mut v) in keyed {
        if c.is_some() && depth < 64 {
            group_sort(&mut v, lnodes, depth + 1);
        }
        items.extend(v);
    }
}

/// Edge crossings between every pair of adjacent ranks.
fn count_crossings(ranks: &[Vec<usize>], lnodes: &[LNode], segs: &[Seg], pos: &[usize]) -> usize {
    let mut total = 0;
    for rk in ranks {
        let mut pairs: Vec<(usize, usize)> = Vec::new();
        for &l in rk {
            for &s in &lnodes[l].downs {
                pairs.push((pos[l], pos[segs[s].down]));
            }
        }
        if pairs.len() < 2 {
            continue;
        }
        pairs.sort_unstable();
        // Count inversions in the lower positions with a Fenwick tree.
        let size = pairs.iter().map(|p| p.1).max().unwrap_or(0) + 2;
        let mut tree = vec![0usize; size + 1];
        for (seen, &(_, b)) in pairs.iter().enumerate() {
            // Number already inserted with lower position <= b.
            let mut i = b + 1;
            let mut le = 0;
            while i > 0 {
                le += tree[i];
                i &= i - 1;
            }
            total += seen - le;
            let mut i = b + 1;
            while i <= size {
                tree[i] += 1;
                i += i & i.wrapping_neg();
            }
        }
    }
    total
}
