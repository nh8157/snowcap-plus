use std::collections::{HashMap, HashSet};
use std::hash::Hash;
use std::fmt::Debug;
// use crate::netsim::{Network, config::ConfigModifier};
use crate::parallelism::types::*;

/// This struct defines a parallel executor
#[derive(Debug, Clone)]
pub struct Dag<T> {
    /// A Directed Acyclic Graph object holding all nodes
    dag: HashMap<T, Node<T>>,
}

impl<T: Eq + Hash + Debug + Copy> Dag<T> {
    pub(crate) fn new() -> Self {
        Self { dag: HashMap::new(), }
    }

    pub(crate) fn insert_node(&mut self, nid: T) -> Option<T> {
        if !self.dag.contains_key(&nid) {
            self.dag.insert(nid, Node::new(nid));
            return None;
        }
        Some(nid)
    }

    pub(crate) fn has_node(&self, node: &T) -> bool {
        self.dag.contains_key(node)
    }

    pub(crate) fn add_dependency(&mut self, from: T, to: T) -> Result<(), DagError<T>> {
        match (self.dag.get(&from), self.dag.get(&to)) {
            (Some(_), Some(_)) => {
                self.dag.get_mut(&from).unwrap().add_next(to);
                self.dag.get_mut(&to).unwrap().add_prev(from);
            },
            (None, _) => return Err(DagError::NodeDoesNotExist(from)),
            (_, None) => return Err(DagError::NodeDoesNotExist(to)),
        }
        Ok(())
    }

    // Returns true if the dag has cycle, false otherwise
    // Use DFS to traverse the dag
    pub(crate) fn check_cycle(&self) -> Result<(), DagError<T>> {
        let ready = self.get_starter_nodes();
        if ready.is_none() {
            return Err(DagError::DagHasCycle);
        }
        let mut visited = HashSet::new();
        for task in ready.unwrap() {
            let res = self.dfs_check_cycle(task, &mut visited);
            if res.is_err() {
                return res;
            }
        }
        Ok(())
    }

    fn dfs_check_cycle(&self, current: T, visited: &mut HashSet<T>) -> Result<(), DagError<T>> {
        if let Some(node) = self.dag.get(&current) {
            let nexts = node.get_next();
            if nexts.len() == 0 {
                // Reached the deepest level without incurring a cycle
                // println!("Reached the deepest level");
                return Ok(());
            }
            if !visited.insert(current) {
                // println!("Visited");
                return Err(DagError::DagHasCycle);
            }
            println!("{:?}", current);
            for node in &nexts {
                let res = self.dfs_check_cycle(*node, visited);
                if res.is_err() {
                    // Some error occurred
                    return res;
                }
            }
            visited.remove(&current);
            return Ok(());
        }
        Err(DagError::NodeDoesNotExist(current))
    }

    /// This function appends the nodes that don't have prev into the ready field
    pub(crate) fn get_starter_nodes(&self) -> Option<Vec<T>> {
        let mut ready = Vec::new();
        for (nid, node) in &self.dag {
            if node.get_prev().len() == 0 {
                ready.push(*nid);
            }
        }
        // println!("ready: {:?}", ready);
        if ready.len() == 0 {
            return None;
        }
        Some(ready)
    }

    pub(crate) fn get_next_of_node(&self, node: T) -> Result<Vec<T>, DagError<T>> {
        match self.dag.get(&node) {
            Some(obj) => return Ok(obj.get_next()),
            None => return Err(DagError::NodeDoesNotExist(node)),
        }
    }

}

#[derive(Debug, Clone)]
struct Node<T> {
    id: T,
    prev_count: usize,
    prev: HashSet<T>,
    next: HashSet<T>,
}

impl<T: Eq + Hash + Debug + Copy> Node<T> {
    fn new(id: T) -> Self {
        Self { id: id, prev_count: 0, prev: HashSet::new(), next: HashSet::new() }
    }

    fn get_id(&self) -> T {
        self.id
    }

    fn get_status(&self) -> bool {
        self.prev_count == self.prev.len()
    }

    fn get_prev(&self) -> Vec<T> {
        self.prev.iter().map(|x| *x).collect()
    }

    fn get_next(&self) -> Vec<T> {
        self.next.iter().map(|x| *x).collect()
    }

    /// Returns true if the node does not exist in the set prev
    fn add_prev(&mut self, node: T) -> Option<T> {
        if self.prev.insert(node) {
            return None;
        }
        Some(node)
    }

    /// Returns true if the node does not exist in the set next
    fn add_next(&mut self, node: T) -> Option<T> {
        if self.next.insert(node) {
            return None;
        }
        Some(node)
    }

    /// This function is invoked by the prev node when it is completed.
    fn _mark_prev_complete(&mut self, node: T) -> Option<bool> {
        if self.prev.contains(&node) {
            self.prev_count += 1;
            return Some(self.get_status());
        }
        // This node does not belong to the prev
        None
    }
}

#[cfg(test)]
mod test {
    use crate::parallelism::dag::*;
    #[test]
    fn test_node_init() {
        let mut node1 = Node::new(0);
        let mut node2 = Node::new(1);
        node1.add_next(node2.get_id());
        node2.add_prev(node1.get_id());
    }
    #[test]
    fn test_node_add() {
        let mut nodes: Vec<_> = (0..10).map(|x| Node::new(x)).collect();
        for nid in 0..(nodes.len() - 1) {
            let current_id = nodes[nid].get_id();
            let next_id = nodes[nid + 1].get_id();
            assert_eq!(nodes[nid].add_next(next_id), None);
            assert_eq!(nodes[nid + 1].add_prev(current_id), None);
        }
    }
    #[test]
    fn test_parallel_executor_init() {
        let mut executor = Dag::new();
        for i in 0..10 {
            assert_eq!(executor.insert_node(i as usize), None);
        }
        for i in 0..9 {
            assert_eq!(executor.add_dependency(i, i + 1).is_ok(), true);
        }
    }
    #[test]
    fn test_no_cycle() {
        let mut executor = Dag::new();
        for i in 0..10 {
            assert_eq!(executor.insert_node(i as usize), None);
        }
        for i in 0..9 {
            assert_eq!(executor.add_dependency(i, i + 1).is_ok(), true);
        }

        assert_eq!(executor.check_cycle().is_ok(), true);
    }
    #[test]
    fn test_has_cycle_1() {
        let mut executor = Dag::new();
        for i in 0..10 {
            assert_eq!(executor.insert_node(i as usize), None);
        }
        for i in 0..10 {
            assert_eq!(executor.add_dependency(i, (i + 1) % 10).is_ok(), true);
        }
        // println!("{:?}", executor);
        assert_eq!(executor.check_cycle().is_ok(), false);
    }
    #[test]
    fn test_has_cycle_2() {
        let mut executor = Dag::new();
        for i in 0..10 {
            assert_eq!(executor.insert_node(i as usize), None);
        }
        for i in 0..9 {
            assert_eq!(executor.add_dependency(i, (i + 1) % 10).is_ok(), true);
        }
        assert_eq!(executor.add_dependency(4, 3).is_ok(), true);

        assert!(executor.check_cycle().is_err());
    }
}
