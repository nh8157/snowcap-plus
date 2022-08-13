use crate::netsim::config::ConfigModifier;
use crate::netsim::Network;
use std::collections::{HashMap, HashSet};
// use crate::Error;

type NodeId = usize;

/// This struct defines a parallel executor
#[derive(Debug)]
pub struct ParallelExecutor {
    /// A Directed Acyclic Graph object holding all nodes
    dag: HashMap<NodeId, ParallelNode>,
    /// A hashset that points to all the ready tasks
    ready: Vec<NodeId>,
}

impl ParallelExecutor {
    pub(crate) fn new() -> Self {
        Self { dag: HashMap::new(), ready: Vec::new() }
    }

    pub(crate) fn insert_node(&mut self, nid: NodeId) -> Option<NodeId> {
        if !self.dag.contains_key(&nid) {
            self.dag.insert(nid, ParallelNode::new(nid));
            return None;
        }
        Some(nid)
    }

    pub(crate) fn add_dependency(&mut self, from: NodeId, to: NodeId) -> Result<(), ParallelError> {
        match (self.dag.get(&from), self.dag.get(&to)) {
            (Some(_), Some(_)) => {
                self.dag.get_mut(&from).unwrap().add_next(to);
                self.dag.get_mut(&to).unwrap().add_prev(from);
            }
            _ => {
                // At least one node does not exist in the dag
                // return an error
                return Err(ParallelError::NodeDoesNotExist);
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
        let mut visited = HashSet::new();
        for task in ready.unwrap() {
            let res = self.dag_dfs(task, &mut visited);
            if res.is_err() {
                return res;
            }
        }
        Ok(())
    }

    fn dag_dfs(&self, current: NodeId, visited: &mut HashSet<NodeId>) -> Result<(), ParallelError> {
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
    fn get_initial_tasks(&self) -> Option<Vec<NodeId>> {
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

    pub(crate) fn execute_maximum_depth(
        &mut self,
        net: &mut Network,
        configs: &Vec<ConfigModifier>,
    ) -> Result<(), ParallelError> {
        // Step 1: Check if the executor has any configuration and the configurations are not forming a cycle
        if self.dag.len() == 0 || self.check_cycle().is_err() {
            return Err(ParallelError::ExecutionFailed);
        }
        // Step 2: Prepare a set of ready task
        match self.get_initial_tasks() {
            Some(tasks) => self.ready = tasks.into_iter().collect(),
            None => return Err(ParallelError::ExecutionFailed),
        }
        
        // Step 3: Begin execution, stop when the ready queue has a zero length
        // We may not need an actual multi-threaded application
        // We just need to execute DFS, and on each branch calculate the execution duration,
        // use the longest duration as the estimate of actual execution time

        // let mut finished_tasks = HashSet::<NodeId>::new();
        while self.ready.len() > 0 {

        }
        Ok(())
    }

    pub(crate) fn execute_single_threaded(
        &mut self,
        net: &mut Network,
    ) {

    }

}

#[derive(Debug)]
struct ParallelNode {
    id: NodeId,
    prev_count: NodeId,
    prev: HashSet<NodeId>,
    next: HashSet<NodeId>,
}

impl ParallelNode {
    fn new(id: NodeId) -> Self {
        Self { id: id, prev_count: 0, prev: HashSet::new(), next: HashSet::new() }
    }

    fn get_id(&self) -> NodeId {
        self.id
    }

    fn get_status(&self) -> bool {
        self.prev_count == self.prev.len()
    }

    fn get_prev(&self) -> Vec<NodeId> {
        self.prev.iter().map(|x| *x).collect()
    }

    fn get_next(&self) -> Vec<NodeId> {
        self.next.iter().map(|x| *x).collect()
    }

    /// Returns true if the node does not exist in the set prev
    fn add_prev(&mut self, node: NodeId) -> Option<NodeId> {
        if !self.prev.insert(node) {
            return None;
        }
        Some(node)
    }

    /// Returns true if the node does not exist in the set next
    fn add_next(&mut self, node: NodeId) -> Option<NodeId> {
        if !self.next.contains(&node) {
            self.next.insert(node);
            return None;
        }
        Some(node)
    }

    /// This function is invoked by the prev node when it is completed.
    fn mark_prev_complete(&mut self, node: NodeId) -> Option<bool> {
        if self.prev.contains(&node) {
            self.prev_count += 1;
            return Some(self.get_status());
        }
        // This node does not belong to the prev
        None
    }
}

pub enum ParallelError {
    NodeDoesNotExist,
    DagHasCycle,
    ExecutionFailed,
}

#[cfg(test)]
mod test {
    use crate::parallelism::parallel_executor::*;
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
    fn test_has_cycle_1() {
        let mut executor = ParallelExecutor::new();
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
        let mut executor = ParallelExecutor::new();
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
