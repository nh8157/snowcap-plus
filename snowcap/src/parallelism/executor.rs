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
        info!("{:?}", dag);

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
        let start_time = Instant::now();
        // execute this node on the graph
        net.apply_modifier(&configs[node]).unwrap();
        let end_time = start_time.elapsed();
        let mut max_node = node;
        let mut max_time = end_time;
        for n in dag.get_next_of_node(node)? {
            let future_time = MaxDepthExec::dfs_traversal(n, dag, configs, net)?;
            if end_time + future_time > max_time {
                max_time = end_time + future_time;
                max_node = n;
            }
        }
        println!("{:?}: {:?}", node, max_node);
        net.undo_action().unwrap();
        Ok(max_time)
    }
}

#[cfg(test)]
mod test {
    use crate::example_networks::{ExampleNetwork, ChainGadgetLegacy};
    use crate::example_networks::repetitions::*;
    use crate::dep_groups::strategy_zone::StrategyZone;
    use crate::strategies::{StrategyDAG, StrategyTRTA, Strategy};
    use crate::Stopper;
    use crate::parallelism::executor::{MaxDepthExec, Executor};
    use crate::parallelism::{Dag, ConfigId};

    #[test]
    fn test_dfs_traversal() {
        let net = ChainGadgetLegacy::<Repetition10>::net(0);
        let mut net_exec = net.clone();
        let initial_config = ChainGadgetLegacy::<Repetition10>::initial_config(&net, 0);
        let end_config = ChainGadgetLegacy::<Repetition10>::final_config(&net, 0);
        let end_config_exec = end_config.clone();
        let hard_policy = ChainGadgetLegacy::<Repetition10>::get_policy(&net, 0);
        let dag = StrategyZone::synthesize(net, end_config, hard_policy, None, Stopper::new()).unwrap();

        let time = MaxDepthExec::execute(&dag, &initial_config.get_diff(&end_config_exec).modifiers, &mut net_exec).unwrap();
        println!("{:?}", time);
    }

    #[test]
    fn test_linear_traversal() {
        let net = ChainGadgetLegacy::<Repetition10>::net(0);
        let mut net_exec = net.clone();
        // let initial_config = ChainGadgetLegacy::<Repetition10>::initial_config(&net, 0);
        let end_config = ChainGadgetLegacy::<Repetition10>::final_config(&net, 0);
        let hard_policy = ChainGadgetLegacy::<Repetition10>::get_policy(&net, 0);
        let order = StrategyTRTA::synthesize(net, end_config, hard_policy, None, Stopper::new()).unwrap();
        let mut dag = Dag::<ConfigId>::new();
        
        for i in 0..(order.len() - 1) {
            dag.insert_node(i);
            dag.insert_node(i + 1);
            dag.add_dependency(i, i + 1).unwrap();
        }

        println!("{:?}", dag);

        let time = MaxDepthExec::execute(&dag, &order, &mut net_exec).unwrap();
        println!("{:?}", time);
    }
}
