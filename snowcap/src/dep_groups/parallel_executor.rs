use std::collections::{HashMap, HashSet};
use crate::netsim::Network;
use crate::{Error, error};

type NodeId = usize;

#[derive(Debug)]
pub struct ParallelExecutor {
    /// A Directed Acyclic Graph object holding all nodes
    dag: HashMap<usize, ParallelNode>,
    /// A hashset that points to all the ready tasks
    ready: HashSet<usize>,
}

impl ParallelExecutor {
    fn new() -> Self {
        Self { dag: HashMap::new(), ready: HashSet::new() }
    }

    fn insert_node(&mut self, nid: usize) -> Option<usize> {
        if !self.dag.contains_key(&nid) {
            self.dag.insert(nid, ParallelNode::new(nid));
            return None;
        }
        Some(nid)
    }

    fn add_dependency(&mut self, from: usize, to: usize) -> Result<(), Error> {
        match (self.dag.get(&from), self.dag.get(&to)) {
            (Some(_), Some(_)) => {
                let from_node = self.dag.get_mut(&from).unwrap();
                from_node.add_next(to);
                let to_node = self.dag.get_mut(&to).unwrap();
                to_node.add_prev(from);
            },
            _ => {
                // At least one node does not exist in the dag
                // return an error
            }
        }
        Ok(())
    }

    // Returns true if the dag has cycle, false otherwise
    // Use DFS to traverse the dag
    fn check_cycle(&self) -> Result<(), ParallelError> {
        let ready = self.get_initial_tasks();
        if ready.is_none() {
            return Err(ParallelError::DagHasCycle);
        }
        println!("{:?}", ready);
        let mut visited = HashSet::new();
        for task in ready.unwrap() {
            let res = self.dag_dfs(task, &mut visited);
            if res.is_err() {
                return res;
            }
        }
        Ok(())
    }

    fn dag_dfs(&self, current: usize, visited: &mut HashSet<usize>) -> Result<(), ParallelError> {
        if let Some(node) = self.dag.get(&current) {
            let nexts = node.get_next();
            if nexts.len() == 0 {
                // Reached the deepest level without incurring a cycle
                println!("Reached the deepest level");
                return Ok(());
            }
            if !visited.insert(current) {
                println!("Visited");
                return Err(ParallelError::DagHasCycle);
            }
            println!("{}", current);
            for node in &nexts {
                let res = self.dag_dfs(*node, visited); 
                if res.is_err() {
                    // Some error occurred
                    return res;
                }
            }
            visited.remove(&current);
            return Ok(());
        }
        Err(ParallelError::NodeDoesNotExist)
    }

    /// This function appends the nodes that don't have prev into the ready field
    fn get_initial_tasks(&self) -> Option<Vec<usize>> {
        let mut ready = Vec::new();
        for (nid, node) in &self.dag {
            if node.get_prev().len() == 0 {
                ready.push(*nid);
            }
        }
        if ready.len() == 0 {
            return None
        }
        Some(ready)
    }

    fn execute(&mut self, net: &mut Network) -> Result<(), Error> {
        Err(Error::NotImplemented)
    }
}

#[derive(Debug)]
struct ParallelNode {
    id: usize,
    prev_count: usize,
    prev: HashMap<usize, bool>,
    next: HashMap<usize, bool>,
}

impl ParallelNode {
    fn new(id: usize) -> Self {
        Self { id: id, prev_count: 0, prev: HashMap::new(), next: HashMap::new() }
    }

    fn get_id(&self) -> usize {
        self.id
    }

    fn get_status(&self) -> bool {
        self.prev_count == self.prev.len()
    }

    fn get_prev(&self) -> Vec<usize> {
        self.prev.keys().map(|x| *x).collect()
    }

    fn get_next(&self) -> Vec<usize> {
        self.next.keys().map(|x| *x).collect()
    }

    /// Returns true if the node does not exist in the set prev
    fn add_prev(&mut self, node: usize) -> Option<usize> {
        if !self.prev.contains_key(&node) {
            self.prev.insert(node, false);
            return None;
        }
        Some(node)
    }

    /// Returns true if the node does not exist in the set next
    fn add_next(&mut self, node: usize) -> Option<usize> {
        if !self.next.contains_key(&node) {
            self.next.insert(node, false);
            return None;
        }
        Some(node)
    }

    /// This function is invoked by the prev node when it is completed.
    fn mark_prev_complete(&mut self, node: usize) -> Option<bool> {
        if self.prev.contains_key(&node) {
            let ptr = self.prev.get_mut(&node).unwrap();
            if !*ptr {
                *ptr = true;
                self.prev_count += 1;
            }
            return Some(self.get_status());
        }
        // This node does not belong to the prev
        None
    }
}

#[derive(Debug)]
enum ParallelError {
    NodeDoesNotExist,
    DagHasCycle,

}

#[cfg(test)]
mod test {
    use crate::dep_groups::parallel_executor::*;
    #[test]
    fn test_node_init() {
        let mut node1 = ParallelNode::new(0);
        let mut node2 = ParallelNode::new(1);
        node1.add_next(node2.get_id());
        node2.add_prev(node1.get_id());
    }
    #[test]
    fn test_node_add() {
        let mut nodes: Vec<_> = (0..10).map(|x| ParallelNode::new(x)).collect();
        for nid in 0..(nodes.len() - 1) {
            let current_id = nodes[nid].get_id();
            let next_id = nodes[nid + 1].get_id();
            assert_eq!(nodes[nid].add_next(next_id), None);
            assert_eq!(nodes[nid + 1].add_prev(current_id), None);
        }
    }
    #[test]
    fn test_parallel_executor_init() {
        let mut executor = ParallelExecutor::new();
        for i in 0..10 {
            assert_eq!(executor.insert_node(i as usize), None);
        }
        for i in 0..9 {
            assert_eq!(executor.add_dependency(i, i + 1).is_ok(), true);
        }
    }
    #[test]
    fn test_no_cycle() {
        let mut executor = ParallelExecutor::new();
        for i in 0..10 {
            assert_eq!(executor.insert_node(i as usize), None);
        }
        for i in 0..9 {
            assert_eq!(executor.add_dependency(i, i + 1).is_ok(), true);
        }

        assert_eq!(executor.check_cycle().is_ok(), true);
    }
    #[test]
    fn test_has_cycle() {
        let mut executor = ParallelExecutor::new();
        for i in 0..10 {
            assert_eq!(executor.insert_node(i as usize), None);
        }
        for i in 0..10 {
            assert_eq!(executor.add_dependency(i, (i + 1) % 10).is_ok(), true);
        }
        println!("{:?}", executor);
        assert_eq!(executor.check_cycle().is_ok(), false);
    }
}