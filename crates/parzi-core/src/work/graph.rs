//! A goal's steps are a graph: keys are unique, needs name steps of the same
//! goal, nothing waits on itself in a circle, and a step cannot be done while
//! something it needs is still open. Ported from the task list it replaces.

use std::collections::{HashMap, HashSet};

/// What the graph rules need to know about a step.
pub trait Node {
    fn key(&self) -> &str;
    fn needs(&self) -> &[String];
    fn needs_mut(&mut self) -> &mut Vec<String>;
    /// Done (or skipped): nothing waits on it any longer.
    fn settled(&self) -> bool;
}

impl Node for super::fold::Step {
    fn key(&self) -> &str {
        &self.key
    }
    fn needs(&self) -> &[String] {
        &self.needs
    }
    fn needs_mut(&mut self) -> &mut Vec<String> {
        &mut self.needs
    }
    fn settled(&self) -> bool {
        self.status.settled()
    }
}

/// Short ids people can type: letters and digits with no look-alikes.
#[must_use]
pub fn fresh_id(len: usize, taken: &dyn Fn(&str) -> bool) -> String {
    const ALPHABET: &[u8] = b"abcdefghjkmnpqrstuvwxyz23456789";
    loop {
        let mut id = String::new();
        while id.len() < len {
            for b in uuid::Uuid::new_v4().as_bytes() {
                if id.len() == len {
                    break;
                }
                id.push(ALPHABET[usize::from(*b) % ALPHABET.len()] as char);
            }
        }
        // Never all digits: an id must not read as a count or an index.
        if id.chars().any(|c| c.is_ascii_alphabetic()) && !taken(&id) {
            return id;
        }
    }
}

/// Every rule holds, or the first one broken in words an agent can act on.
pub fn validate<N: Node>(steps: &[N]) -> Result<(), String> {
    let mut seen = HashSet::new();
    for s in steps {
        if !seen.insert(s.key()) {
            return Err(format!("step `{}` is used twice", s.key()));
        }
    }
    let pos: HashMap<&str, usize> = steps
        .iter()
        .enumerate()
        .map(|(i, s)| (s.key(), i))
        .collect();
    for s in steps {
        for need in s.needs() {
            if !pos.contains_key(need.as_str()) {
                return Err(format!(
                    "step `{}` needs `{need}`, which is not a step of this goal",
                    s.key()
                ));
            }
        }
    }
    // Cycle check: iterative DFS, 1 = on the stack, 2 = finished.
    let mut color = vec![0u8; steps.len()];
    for start in 0..steps.len() {
        if color[start] != 0 {
            continue;
        }
        let mut stack: Vec<(usize, bool)> = vec![(start, false)];
        while let Some((i, closing)) = stack.pop() {
            if closing {
                color[i] = 2;
                continue;
            }
            match color[i] {
                2 => continue,
                1 => {
                    return Err(format!(
                        "steps depend on each other in a circle through `{}`",
                        steps[i].key()
                    ))
                }
                _ => {}
            }
            color[i] = 1;
            stack.push((i, true));
            for need in steps[i].needs() {
                if let Some(&j) = pos.get(need.as_str()) {
                    stack.push((j, false));
                }
            }
        }
    }
    for s in steps.iter().filter(|s| s.settled()) {
        let open = open_needs(s, steps);
        if !open.is_empty() {
            return Err(format!(
                "step `{}` cannot be done while it waits on {}",
                s.key(),
                open.join(", ")
            ));
        }
    }
    Ok(())
}

/// The needs of `step` that are not settled yet.
#[must_use]
pub fn open_needs<N: Node>(step: &N, scope: &[N]) -> Vec<String> {
    step.needs()
        .iter()
        .filter(|n| !scope.iter().any(|s| s.key() == n.as_str() && s.settled()))
        .cloned()
        .collect()
}

/// A list made valid without losing a step: needs on missing steps go, a
/// settled step drops needs that are still open, and a need that closes a
/// circle is cut. Repeated keys keep their first copy.
#[must_use]
pub fn sanitize<N: Node>(steps: Vec<N>) -> Vec<N> {
    let mut seen = HashSet::new();
    let mut list: Vec<N> = steps
        .into_iter()
        .filter(|s| seen.insert(s.key().to_string()))
        .collect();
    let keys: HashSet<String> = list.iter().map(|s| s.key().to_string()).collect();
    for s in &mut list {
        let own = s.key().to_string();
        let mut kept = HashSet::new();
        s.needs_mut()
            .retain(|n| *n != own && keys.contains(n) && kept.insert(n.clone()));
    }
    let pos: HashMap<String, usize> = list
        .iter()
        .enumerate()
        .map(|(i, s)| (s.key().to_string(), i))
        .collect();
    let mut color = vec![0u8; list.len()];
    for start in 0..list.len() {
        if color[start] == 0 {
            cut_back_edges(&mut list, &pos, &mut color, start);
        }
    }
    let settled: HashSet<String> = list
        .iter()
        .filter(|s| s.settled())
        .map(|s| s.key().to_string())
        .collect();
    for s in list.iter_mut().filter(|s| s.settled()) {
        s.needs_mut().retain(|n| settled.contains(n));
    }
    list
}

fn cut_back_edges<N: Node>(
    list: &mut [N],
    pos: &HashMap<String, usize>,
    color: &mut [u8],
    start: usize,
) {
    // (step, index of the next need to look at); 1 = on the path, 2 = done.
    let mut stack: Vec<(usize, usize)> = vec![(start, 0)];
    color[start] = 1;
    while let Some(top) = stack.len().checked_sub(1) {
        let (i, next) = stack[top];
        if next >= list[i].needs().len() {
            color[i] = 2;
            stack.pop();
            continue;
        }
        match pos.get(&list[i].needs()[next]).map(|&j| (j, color[j])) {
            Some((_, 1)) => {
                list[i].needs_mut().remove(next);
            }
            Some((j, 0)) => {
                stack[top].1 += 1;
                color[j] = 1;
                stack.push((j, 0));
            }
            _ => stack[top].1 += 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::fold::Step;
    use super::super::model::StepStatus;
    use super::*;

    fn step(key: &str, status: StepStatus, needs: &[&str]) -> Step {
        Step {
            key: key.into(),
            title: key.into(),
            needs: needs.iter().map(|s| (*s).to_string()).collect(),
            status,
            ..Step::default()
        }
    }

    #[test]
    fn the_graph_rules_hold() {
        use StepStatus::{Done, Todo};
        assert!(validate(&[step("a", Todo, &[]), step("b", Todo, &["a"])]).is_ok());
        let twice = validate(&[step("a", Todo, &[]), step("a", Todo, &[])]).unwrap_err();
        assert!(twice.contains("twice"), "{twice}");
        let unknown = validate(&[step("b", Todo, &["a"])]).unwrap_err();
        assert!(unknown.contains("not a step"), "{unknown}");
        let circle = validate(&[step("x", Todo, &["y"]), step("y", Todo, &["x"])]).unwrap_err();
        assert!(circle.contains("circle"), "{circle}");
        let selfish = validate(&[step("x", Todo, &["x"])]).unwrap_err();
        assert!(selfish.contains("circle"), "{selfish}");
        let early = validate(&[step("a", Todo, &[]), step("b", Done, &["a"])]).unwrap_err();
        assert!(early.contains("waits on a"), "{early}");
        assert!(validate(&[step("a", Done, &[]), step("b", Done, &["a"])]).is_ok());
    }

    #[test]
    fn sanitize_mends_what_validate_refuses() {
        use StepStatus::{Done, Todo};
        let list = sanitize(vec![
            step("a", Todo, &[]),
            step("a", Done, &[]),
            step("b", Todo, &["a", "gone", "b", "a"]),
            step("x", Todo, &["y"]),
            step("y", Todo, &["x"]),
            step("d", Done, &["b"]),
        ]);
        assert!(validate(&list).is_ok(), "{:?}", validate(&list));
        let by = |k: &str| list.iter().find(|s| s.key == k).unwrap();
        assert_eq!(list.len(), 5, "only the repeat goes");
        assert_eq!(by("a").status, Todo, "a repeated key keeps its first copy");
        assert_eq!(by("b").needs, vec!["a".to_string()]);
        assert!(by("d").needs.is_empty(), "done drops its open needs");
        assert_eq!(by("x").needs.len() + by("y").needs.len(), 1);
    }

    #[test]
    fn fresh_ids_are_short_and_free() {
        let id = fresh_id(5, &|s| s.starts_with('a'));
        assert_eq!(id.len(), 5);
        assert!(!id.starts_with('a'));
        assert!(id.chars().all(|c| c.is_ascii_alphanumeric()));
        assert_eq!(fresh_id(12, &|_| false).len(), 12);
    }
}
