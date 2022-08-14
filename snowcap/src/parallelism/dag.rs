use std::collections::{HashMap, HashSet};
use std::hash::Hash;
use std::fmt::Debug;
use crate::parallelism::types::*;

/// This struct defines a parallel executor
#[derive(Debug)]
pub struct Dag<T> {
    /// A Directed Acyclic Graph object holding all nodes
    dag: HashMap<T, Node<T>>,
    // /// A hashset that points to all the ready tasks
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
    fn check_cycle(&self) -> Result<(), DagError<T>> {
        let ready = self.get_starter_nodes();
        if ready.is_none() {
            return Err(DagError::DagHasCycle);
        }
        let mut visited = HashSet::new();
        for task in ready.unwrap() {
            let res = self.dag_dfs(task, &mut visited);
            if res.is_err() {
                return res;
            }
        }
        Ok(())
    }

    fn dag_dfs(&self, current: T, visited: &mut HashSet<T>) -> Result<(), DagError<T>> {
        if let Some(node) = self.dag.get(&current) {
            let nexts = node.get_next();
            if nexts.len() == 0 {
                // Reached the deepest level without incurring a cycle
                println!("Reached the deepest level");
                return Ok(());
            }
            if !visited.insert(current) {
                println!("Visited");
                return Err(DagError::DagHasCycle);
            }
            println!("{:?}", current);
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
        Err(DagError::NodeDoesNotExist(current))
    }

    /// This function appends the nodes that don't have prev into the ready field
    fn get_starter_nodes(&self) -> Option<Vec<T>> {
        let mut ready = Vec::new();
        for (nid, node) in &self.dag {
            if node.get_prev().len() == 0 {
                ready.push(*nid);
            }
        }
        if ready.len() == 0 {
            return None;
        }
        Some(ready)
    }

    // pub(crate) fn execute_maximum_depth(
    //     &mut self,
    //     net: &mut Network,
    //     configs: &Vec<ConfigModifier>,
    // ) -> Result<(), ParallelError> {
    //     // Step 1: Check if the executor has any configuration and the configurations are not forming a cycle
    //     if self.dag.len() == 0 || self.check_cycle().is_err() {
    //         return Err(ParallelError::ExecutionFailed);
    //     }
    //     // Step 2: Prepare a set of ready task
    //     match self.get_initial_tasks() {
    //         Some(tasks) => {},
    //         None => return Err(ParallelError::ExecutionFailed),
    //     }
        
    //     // Step 3: Begin execution, stop when the ready queue has a zero length
    //     // We may not need an actual multi-threaded application
    //     // We just need to execute DFS, and on each branch calculate the execution duration,
    //     // use the longest duration as the estimate of actual execution time

    //     // let mut finished_tasks = HashSet::<T>::new();
    //     // while self.ready.len() > 0 {

    //     // }
    //     Ok(())
    // }

    // pub(crate) fn execute_single_threaded(
    //     &mut self,
    //     net: &mut Network,
    // ) {

    // }

}

#[derive(Debug)]
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
    fn mark_prev_complete(&mut self, node: T) -> Option<bool> {
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
