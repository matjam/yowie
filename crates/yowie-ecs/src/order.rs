//! Ordering constraints between systems of one stage.
//!
//! A system names the systems or sets it must run before or after
//! ([`System::before`](crate::System::before),
//! [`System::after`](crate::System::after)), and the sets it belongs to
//! ([`System::in_set`](crate::System::in_set)). A stage runs in a stable
//! topological order: systems with no constraint between them keep the
//! order they were added in.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

/// What the sort reads of one system.
pub(crate) struct Node<'a> {
    pub(crate) name: &'a str,
    pub(crate) before: &'a [String],
    pub(crate) after: &'a [String],
    pub(crate) sets: &'a [String],
}

impl Node<'_> {
    /// Whether `target` names this system or one of its sets.
    pub(crate) fn matches(&self, target: &str) -> bool {
        self.name == target || self.sets.iter().any(|s| s == target)
    }
}

/// Sorts `nodes` so every constraint holds, taking the lowest index among
/// the systems that are ready at each step. Constraints that name nothing
/// in `nodes` are ignored here (the schedule reports unknown names
/// separately). Returns the indices in run order, or on a cycle the
/// indices of the systems that could not be placed.
pub(crate) fn sort(nodes: &[Node<'_>]) -> Result<Vec<usize>, Vec<usize>> {
    let n = nodes.len();
    let mut successors: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut waiting = vec![0_usize; n];
    let mut edge = |from: usize, to: usize| {
        if from != to && !successors[from].contains(&to) {
            successors[from].push(to);
            waiting[to] += 1;
        }
    };
    for (i, node) in nodes.iter().enumerate() {
        for target in node.before {
            for (j, other) in nodes.iter().enumerate() {
                if other.matches(target) {
                    edge(i, j);
                }
            }
        }
        for target in node.after {
            for (j, other) in nodes.iter().enumerate() {
                if other.matches(target) {
                    edge(j, i);
                }
            }
        }
    }
    let mut ready: BinaryHeap<Reverse<usize>> = (0..n)
        .filter(|i| waiting[*i] == 0)
        .map(Reverse)
        .collect();
    let mut order = Vec::with_capacity(n);
    while let Some(Reverse(i)) = ready.pop() {
        order.push(i);
        for &j in &successors[i] {
            waiting[j] -= 1;
            if waiting[j] == 0 {
                ready.push(Reverse(j));
            }
        }
    }
    if order.len() == n {
        Ok(order)
    } else {
        Err((0..n).filter(|i| waiting[*i] > 0).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Spec {
        name: &'static str,
        before: Vec<String>,
        after: Vec<String>,
        sets: Vec<String>,
    }

    fn spec(name: &'static str, before: &[&str], after: &[&str], sets: &[&str]) -> Spec {
        let own = |v: &[&str]| v.iter().map(|s| (*s).to_owned()).collect();
        Spec {
            name,
            before: own(before),
            after: own(after),
            sets: own(sets),
        }
    }

    fn run(specs: &[Spec]) -> Result<Vec<&'static str>, Vec<&'static str>> {
        let nodes: Vec<Node> = specs
            .iter()
            .map(|s| Node {
                name: s.name,
                before: &s.before,
                after: &s.after,
                sets: &s.sets,
            })
            .collect();
        sort(&nodes)
            .map(|o| o.into_iter().map(|i| specs[i].name).collect())
            .map_err(|o| o.into_iter().map(|i| specs[i].name).collect())
    }

    #[test]
    fn unconstrained_systems_keep_insertion_order() {
        let s = [
            spec("a", &[], &[], &[]),
            spec("b", &[], &[], &[]),
            spec("c", &[], &[], &[]),
        ];
        assert_eq!(run(&s), Ok(vec!["a", "b", "c"]));
    }

    #[test]
    fn constraints_move_only_what_they_must() {
        let cases: [(Vec<Spec>, Vec<&str>); 4] = [
            // c must run first.
            (
                vec![
                    spec("a", &[], &[], &[]),
                    spec("b", &[], &[], &[]),
                    spec("c", &["a"], &[], &[]),
                ],
                vec!["b", "c", "a"],
            ),
            // a after c.
            (
                vec![
                    spec("a", &[], &["c"], &[]),
                    spec("b", &[], &[], &[]),
                    spec("c", &[], &[], &[]),
                ],
                vec!["b", "c", "a"],
            ),
            // A set is a group target.
            (
                vec![
                    spec("render", &[], &["logic"], &[]),
                    spec("move", &[], &[], &["logic"]),
                    spec("score", &[], &[], &["logic"]),
                ],
                vec!["move", "score", "render"],
            ),
            // A chain.
            (
                vec![
                    spec("c", &[], &["b"], &[]),
                    spec("b", &[], &["a"], &[]),
                    spec("a", &[], &[], &[]),
                ],
                vec!["a", "b", "c"],
            ),
        ];
        for (specs, want) in cases {
            assert_eq!(run(&specs), Ok(want));
        }
    }

    #[test]
    fn a_cycle_names_the_stuck_systems() {
        let s = [
            spec("free", &[], &[], &[]),
            spec("a", &["b"], &[], &[]),
            spec("b", &["c"], &[], &[]),
            spec("c", &["a"], &[], &[]),
        ];
        assert_eq!(run(&s), Err(vec!["a", "b", "c"]));
    }
}
