use crate::netsim::config::Config;
use crate::parallelism::{Dag, DagError, ExecutorError, ConfigId};
use crate::netsim::{Network, config::ConfigModifier};
use std::time::{Instant, Duration};
use log::info;

pub trait Executor {
    fn execute(dag: &Dag<ConfigId>, configs: &Vec<ConfigModifier>, net: &mut Network) -> Result<Duration, ExecutorError>;
}

pub struct MaxDepthExec {}

impl Executor for MaxDepthExec {
    fn execute(dag: &Dag<ConfigId>, configs: &Vec<ConfigModifier>, net: &mut Network) -> Result<Duration, ExecutorError> {
        // first validate if the dag has a cycle
        dag.check_cycle()?;

        if let Some(nodes) = dag.get_starter_nodes() {
            let mut max_time = Duration::new(0, 0);
            for node in nodes {
                info!("Traversal begins");
                let exec_time = MaxDepthExec::dfs_traversal(node, dag, configs, net)?;
                max_time = max_time.max(exec_time);
                info!("Traversal ends");
            }
            return Ok(max_time);
        }
        Err(ExecutorError::ExecutionFailed)
    }
}

impl MaxDepthExec {
    fn dfs_traversal(node: ConfigId, dag: &Dag<ConfigId>, configs: &Vec<ConfigModifier>, net: &mut Network) -> Result<Duration, DagError<ConfigId>> {
        info!("Node {}", node);
        let start_time = Instant::now();
        // execute this node on the graph
        net.apply_modifier(&configs[node]);
        let end_time = start_time.elapsed();
        let mut max_time = end_time;
        for n in dag.get_next_of_node(node)? {
            let future_time = MaxDepthExec::dfs_traversal(n, dag, configs, net)?;
            if end_time + future_time > max_time {
                max_time = end_time + future_time;
            }
        }
        Ok(max_time)
    }
}