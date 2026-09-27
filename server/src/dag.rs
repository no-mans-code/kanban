//! Dependency-graph checks over `blocks` edges: `(source, target)` means the
//! target cannot finish before the source. Height is the number of tickets on
//! the longest chain, so a single ticket with no links has height 1.
//!
//! Everything is iterative (Kahn's algorithm / BFS) so deep chains cannot
//! overflow the stack.

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};

pub type Edge = (i64, i64);

pub struct Graph {
    out: BTreeMap<i64, Vec<i64>>,
    inc: BTreeMap<i64, Vec<i64>>,
    nodes: BTreeSet<i64>,
}

impl Graph {
    pub fn new(edges: &[Edge]) -> Self {
        let mut out: BTreeMap<i64, Vec<i64>> = BTreeMap::new();
        let mut inc: BTreeMap<i64, Vec<i64>> = BTreeMap::new();
        let mut nodes = BTreeSet::new();
        for &(s, t) in edges {
            out.entry(s).or_default().push(t);
            inc.entry(t).or_default().push(s);
            nodes.insert(s);
            nodes.insert(t);
        }
        Graph { out, inc, nodes }
    }

    /// Shortest path `from -> ... -> to` along edge direction, if one exists.
    pub fn path(&self, from: i64, to: i64) -> Option<Vec<i64>> {
        if from == to {
            return Some(vec![from]);
        }
        let mut prev: HashMap<i64, i64> = HashMap::new();
        let mut queue = VecDeque::from([from]);
        while let Some(n) = queue.pop_front() {
            for &m in self.out.get(&n).into_iter().flatten() {
                if m == from || prev.contains_key(&m) {
                    continue;
                }
                prev.insert(m, n);
                if m == to {
                    let mut path = vec![to];
                    let mut cur = to;
                    while let Some(&p) = prev.get(&cur) {
                        path.push(p);
                        cur = p;
                    }
                    path.reverse();
                    return Some(path);
                }
                queue.push_back(m);
            }
        }
        None
    }

    /// The cycle that adding `source -> target` would close, as
    /// `[source, target, ..., source]`, or None if the edge is safe.
    pub fn cycle_if_added(&self, source: i64, target: i64) -> Option<Vec<i64>> {
        if source == target {
            return Some(vec![source, source]);
        }
        let back = self.path(target, source)?;
        let mut cycle = vec![source];
        cycle.extend(back);
        Some(cycle)
    }

    fn topo_order(&self) -> Vec<i64> {
        let mut indeg: BTreeMap<i64, usize> = self.nodes.iter().map(|&n| (n, 0)).collect();
        for targets in self.out.values() {
            for t in targets {
                *indeg.get_mut(t).expect("node registered") += 1;
            }
        }
        let mut queue: VecDeque<i64> = indeg.iter().filter(|(_, d)| **d == 0).map(|(n, _)| *n).collect();
        let mut order = Vec::with_capacity(self.nodes.len());
        while let Some(n) = queue.pop_front() {
            order.push(n);
            for &m in self.out.get(&n).into_iter().flatten() {
                let d = indeg.get_mut(&m).expect("node registered");
                *d -= 1;
                if *d == 0 {
                    queue.push_back(m);
                }
            }
        }
        order
    }

    /// For each node: (length of the longest chain ending at it, predecessor on that chain).
    fn longest_ending(&self, order: &[i64]) -> HashMap<i64, (usize, Option<i64>)> {
        let mut best: HashMap<i64, (usize, Option<i64>)> = HashMap::with_capacity(order.len());
        for &v in order {
            let mut here = (1, None);
            for &u in self.inc.get(&v).into_iter().flatten() {
                let len = best[&u].0 + 1;
                if len > here.0 {
                    here = (len, Some(u));
                }
            }
            best.insert(v, here);
        }
        best
    }

    /// For each node: (length of the longest chain starting at it, successor on that chain).
    fn longest_starting(&self, order: &[i64]) -> HashMap<i64, (usize, Option<i64>)> {
        let mut best: HashMap<i64, (usize, Option<i64>)> = HashMap::with_capacity(order.len());
        for &v in order.iter().rev() {
            let mut here = (1, None);
            for &w in self.out.get(&v).into_iter().flatten() {
                let len = best[&w].0 + 1;
                if len > here.0 {
                    here = (len, Some(w));
                }
            }
            best.insert(v, here);
        }
        best
    }

    fn walk(table: &HashMap<i64, (usize, Option<i64>)>, start: i64) -> Vec<i64> {
        let mut chain = vec![start];
        let mut cur = start;
        while let Some(&(_, Some(next))) = table.get(&cur) {
            chain.push(next);
            cur = next;
        }
        chain
    }

    /// The longest chain that would pass through a new `source -> target`
    /// edge. Only meaningful when `cycle_if_added` returned None.
    pub fn longest_through(&self, source: i64, target: i64) -> Vec<i64> {
        let order = self.topo_order();
        let up = self.longest_ending(&order);
        let down = self.longest_starting(&order);
        let mut chain = if up.contains_key(&source) {
            let mut c = Self::walk(&up, source);
            c.reverse();
            c
        } else {
            vec![source]
        };
        if down.contains_key(&target) {
            chain.extend(Self::walk(&down, target));
        } else {
            chain.push(target);
        }
        chain
    }

    /// The longest chain in the whole graph (empty if there are no edges).
    pub fn longest_chain(&self) -> Vec<i64> {
        let order = self.topo_order();
        let up = self.longest_ending(&order);
        let Some((&end, _)) = up.iter().max_by_key(|(n, (len, _))| (*len, std::cmp::Reverse(**n))) else {
            return Vec::new();
        };
        let mut chain = Self::walk(&up, end);
        chain.reverse();
        chain
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_direct_and_indirect_cycles() {
        let g = Graph::new(&[(1, 2), (2, 3)]);
        assert_eq!(g.cycle_if_added(3, 1), Some(vec![3, 1, 2, 3]));
        assert_eq!(g.cycle_if_added(2, 1), Some(vec![2, 1, 2]));
        assert_eq!(g.cycle_if_added(1, 1), Some(vec![1, 1]));
        assert_eq!(g.cycle_if_added(1, 3), None);
        assert_eq!(g.cycle_if_added(4, 1), None);
    }

    #[test]
    fn diamond_is_not_a_cycle() {
        let g = Graph::new(&[(1, 2), (1, 3), (2, 4)]);
        assert_eq!(g.cycle_if_added(3, 4), None);
    }

    #[test]
    fn longest_chain_prefers_the_longer_branch() {
        let g = Graph::new(&[(1, 2), (2, 3), (3, 4), (1, 5), (5, 4)]);
        assert_eq!(g.longest_chain(), vec![1, 2, 3, 4]);
        assert!(Graph::new(&[]).longest_chain().is_empty());
    }

    #[test]
    fn longest_through_joins_upstream_and_downstream() {
        // 1 -> 2   and   3 -> 4 -> 5 ; adding 2 -> 3 makes a chain of 5
        let g = Graph::new(&[(1, 2), (3, 4), (4, 5)]);
        assert_eq!(g.longest_through(2, 3), vec![1, 2, 3, 4, 5]);
        assert_eq!(g.longest_through(9, 1), vec![9, 1, 2]);
        assert_eq!(g.longest_through(8, 9), vec![8, 9]);
    }

    #[test]
    fn deep_chain_does_not_overflow() {
        let edges: Vec<Edge> = (0..200_000).map(|i| (i, i + 1)).collect();
        let g = Graph::new(&edges);
        assert_eq!(g.longest_chain().len(), 200_001);
        assert!(g.cycle_if_added(200_000, 0).is_some());
    }
}
